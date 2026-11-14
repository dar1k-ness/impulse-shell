//! QtQuick Canvas (Context2D, renderTarget Image): QPainter raster engine on ARGB32_Premultiplied.
//!
//! Path semantics from qquickcontext2d.cpp / qpainterpath.cpp, curve flattening from
//! QOutlineMapper + QBezier::addToPolygon, coverage from qgrayraster.c (lines-only path),
//! span blending from comp_func_solid_SourceOver.

use crate::raster::Canvas;
use crate::shape::{curves_for_arc, ellipse_coords, fcmp_d, fcmp_pt};
use mul255::qt as byte_mul;

mod pathclipper;

type D = [f64; 2];

#[derive(Clone, Copy, Debug)]
pub(crate) enum El {
    Move(D),
    Line(D),
    Cubic(D, D, D),
}

/// Context2D current path (QPainterPath).
#[derive(Default, Clone)]
pub struct Path {
    pub(crate) el: Vec<El>,
    /// QPainterPathPrivate::require_moveTo: the last subpath was closed, so the next line or
    /// curve starts a new one at the current point.
    require_move: bool,
}

fn pt_of(e: &El) -> D {
    match e {
        El::Move(p) | El::Line(p) | El::Cubic(_, _, p) => *p,
    }
}

impl Path {
    pub(crate) fn from_el(el: Vec<El>) -> Path {
        Path {
            el,
            require_move: false,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.el.is_empty()
    }
    /// QPainterPathPrivate::maybeMoveTo.
    fn maybe_move(&mut self) {
        if self.require_move {
            let p = self.cur();
            self.el.push(El::Move(p));
            self.require_move = false;
        }
    }
    fn cur(&self) -> D {
        self.el.last().map(pt_of).unwrap_or([0.0, 0.0])
    }
    fn sub_start(&self) -> D {
        self.el
            .iter()
            .rev()
            .find_map(|e| if let El::Move(p) = e { Some(*p) } else { None })
            .unwrap_or([0.0, 0.0])
    }
    fn p_move(&mut self, p: D) {
        self.require_move = false;
        if let Some(El::Move(_)) = self.el.last() {
            self.el.pop();
        }
        self.el.push(El::Move(p));
    }
    fn p_line(&mut self, p: D) {
        if self.el.is_empty() {
            self.el.push(El::Move([0.0, 0.0]));
        }
        self.maybe_move();
        if fcmp_pt(p, self.cur()) {
            return;
        }
        self.el.push(El::Line(p));
    }
    fn p_cubic(&mut self, c1: D, c2: D, e: D) {
        if self.el.is_empty() {
            self.el.push(El::Move([0.0, 0.0]));
        }
        let l = self.cur();
        if fcmp_pt(l, c1) && fcmp_pt(c1, c2) && fcmp_pt(c2, e) {
            return;
        }
        self.maybe_move();
        self.el.push(El::Cubic(c1, c2, e));
    }
    fn p_close(&mut self) {
        if self.el.is_empty() {
            return;
        }
        self.require_move = true;
        let s = self.sub_start();
        let c = self.cur();
        if s == c {
            return;
        }
        if fcmp_d(s[0], c[0]) && fcmp_d(s[1], c[1]) {
            match self.el.last_mut() {
                Some(El::Move(p)) | Some(El::Line(p)) | Some(El::Cubic(_, _, p)) => *p = s,
                None => {}
            }
        } else {
            self.el.push(El::Line(s));
        }
    }

    /// QPainterPath::closeSubpath.
    pub fn close_subpath(&mut self) {
        self.p_close();
    }

    /// QQuickContext2D::quadraticCurveTo (QPainterPath::quadTo).
    pub fn quad_to(&mut self, cx: f64, cy: f64, x: f64, y: f64) {
        if self.el.is_empty() {
            self.p_move([cx, cy]);
        }
        let prev = self.cur();
        if fcmp_pt(prev, [x, y]) {
            return;
        }
        let c1 = [(prev[0] + 2.0 * cx) / 3.0, (prev[1] + 2.0 * cy) / 3.0];
        let c2 = [(x + 2.0 * cx) / 3.0, (y + 2.0 * cy) / 3.0];
        self.p_cubic(c1, c2, [x, y]);
    }

    /// QPainterPath::addRoundedRect(rect, xr, yr, Qt::AbsoluteSize).
    pub fn add_rounded_rect(&mut self, x: f64, y: f64, w: f64, h: f64, xr: f64, yr: f64) {
        let [x, y, w, h] = normalized([x, y, w, h]);
        if w == 0.0 && h == 0.0 {
            return;
        }
        let (w2, h2) = (w / 2.0, h / 2.0);
        let xr = if w2 == 0.0 {
            0.0
        } else {
            100.0 * xr.min(w2) / w2
        };
        let yr = if h2 == 0.0 {
            0.0
        } else {
            100.0 * yr.min(h2) / h2
        };
        if xr <= 0.0 || yr <= 0.0 {
            return self.add_rect(x, y, w, h);
        }
        let (rxx2, ryy2) = (w * xr / 100.0, h * yr / 100.0);
        // arcMoveTo.
        let r0 = [x, y, rxx2, ryy2];
        if !(rxx2 == 0.0 && ryy2 == 0.0) {
            self.p_move(ellipse_coords(r0, 180.0, 0.0).0);
        }
        self.arc_to(r0, 180.0, -90.0);
        self.arc_to([x + w - rxx2, y, rxx2, ryy2], 90.0, -90.0);
        self.arc_to([x + w - rxx2, y + h - ryy2, rxx2, ryy2], 0.0, -90.0);
        self.arc_to([x, y + h - ryy2, rxx2, ryy2], 270.0, -90.0);
        self.p_close();
    }

    /// QPainterPath::addEllipse.
    pub fn add_ellipse(&mut self, x: f64, y: f64, w: f64, h: f64) {
        if w == 0.0 && h == 0.0 {
            return;
        }
        let (start, cv) = curves_for_arc([x, y, w, h], 0.0, -360.0);
        self.p_move(start);
        for k in cv.chunks(3) {
            self.p_cubic(k[0], k[1], k[2]);
        }
        self.require_move = true;
    }

    /// QPainterPath::addPath.
    pub fn add_path(&mut self, other: &Path) {
        if other.el.is_empty() {
            return;
        }
        if let Some(El::Move(_)) = self.el.last() {
            self.el.pop();
        }
        self.el.extend(other.el.iter().copied());
        self.require_move = other.sub_start() == other.cur();
    }

    /// QPainterPath::controlPointRect as x0, y0, x1, y1.
    pub fn control_point_rect(&self) -> Option<[f64; 4]> {
        let mut b: Option<[f64; 4]> = None;
        let mut add = |p: D| {
            let r = b.get_or_insert([p[0], p[1], p[0], p[1]]);
            *r = [
                r[0].min(p[0]),
                r[1].min(p[1]),
                r[2].max(p[0]),
                r[3].max(p[1]),
            ];
        };
        for e in &self.el {
            match *e {
                El::Move(p) | El::Line(p) => add(p),
                El::Cubic(a, c, d) => {
                    add(a);
                    add(c);
                    add(d);
                }
            }
        }
        b
    }

    /// QPainterPath::boundingRect as x, y, w, h: curves count with their extrema.
    pub fn bounding_rect(&self) -> [f64; 4] {
        let mut b: Option<[f64; 4]> = None;
        let mut add = |p: D| {
            let r = b.get_or_insert([p[0], p[1], p[0], p[1]]);
            *r = [
                r[0].min(p[0]),
                r[1].min(p[1]),
                r[2].max(p[0]),
                r[3].max(p[1]),
            ];
        };
        let mut last = [0.0, 0.0];
        for e in &self.el {
            match *e {
                El::Move(p) | El::Line(p) => add(p),
                El::Cubic(c1, c2, p) => {
                    add(p);
                    // qt_painterpath_bezier_extrema: the roots of the derivative on each axis.
                    for k in 0..2 {
                        let (p0, p1, p2, p3) = (last[k], c1[k], c2[k], p[k]);
                        let a = 3.0 * (-p0 + 3.0 * p1 - 3.0 * p2 + p3);
                        let bq = 6.0 * (p0 - 2.0 * p1 + p2);
                        let c = 3.0 * (p1 - p0);
                        let mut ts = [f64::NAN; 2];
                        if a.abs() < 1e-12 {
                            if bq.abs() > 1e-12 {
                                ts[0] = -c / bq;
                            }
                        } else {
                            let disc = bq * bq - 4.0 * a * c;
                            if disc >= 0.0 {
                                let r = disc.sqrt();
                                ts = [(-bq + r) / (2.0 * a), (-bq - r) / (2.0 * a)];
                            }
                        }
                        for t in ts {
                            if t > 0.0 && t < 1.0 {
                                let u = 1.0 - t;
                                let at = |q: usize| {
                                    u * u * u * last[q]
                                        + 3.0 * u * u * t * c1[q]
                                        + 3.0 * u * t * t * c2[q]
                                        + t * t * t * p[q]
                                };
                                add([at(0), at(1)]);
                            }
                        }
                    }
                }
            }
            last = pt_of(e);
        }
        match b {
            Some(r) => [r[0], r[1], r[2] - r[0], r[3] - r[1]],
            None => [0.0; 4],
        }
    }

    /// QPainterPath::contains(point) with the given fill rule.
    pub fn contains(&self, pt: D, even_odd: bool) -> bool {
        let Some(r) = self.control_point_rect() else {
            return false;
        };
        // QRectF::contains: a null rect holds nothing.
        if (r[2] - r[0] == 0.0 && r[3] - r[1] == 0.0)
            || pt[0] < r[0]
            || pt[0] > r[2]
            || pt[1] < r[1]
            || pt[1] > r[3]
        {
            return false;
        }
        fn line(p1: D, p2: D, pos: D, winding: &mut i32) {
            let (mut x1, mut y1, mut x2, mut y2) = (p1[0], p1[1], p2[0], p2[1]);
            let mut dir = 1;
            if fcmp_d(y1, y2) || y1 == y2 {
                return;
            } else if y2 < y1 {
                std::mem::swap(&mut x1, &mut x2);
                std::mem::swap(&mut y1, &mut y2);
                dir = -1;
            }
            if pos[1] >= y1 && pos[1] < y2 {
                let x = x1 + ((x2 - x1) / (y2 - y1)) * (pos[1] - y1);
                if x <= pos[0] {
                    *winding += dir;
                }
            }
        }
        fn curve(b: [D; 4], pt: D, winding: &mut i32, depth: i32) {
            let ys = b.map(|p| p[1]);
            let xs = b.map(|p| p[0]);
            let (y0, y1) = (
                ys.iter().copied().fold(f64::MAX, f64::min),
                ys.iter().copied().fold(f64::MIN, f64::max),
            );
            let (x0, x1) = (
                xs.iter().copied().fold(f64::MAX, f64::min),
                xs.iter().copied().fold(f64::MIN, f64::max),
            );
            if pt[1] >= y0 && pt[1] < y1 {
                if depth == 32 || (x1 - x0 < 0.001 && y1 - y0 < 0.001) {
                    if b[0][0] <= pt[0] {
                        *winding += if b[3][1] > b[0][1] { 1 } else { -1 };
                    }
                    return;
                }
                let (l, r) = bez_split(b);
                curve(l, pt, winding, depth + 1);
                curve(r, pt, winding, depth + 1);
            }
        }
        let mut winding = 0;
        let (mut last, mut start) = ([0.0, 0.0], [0.0, 0.0]);
        for (i, e) in self.el.iter().enumerate() {
            match *e {
                El::Move(p) => {
                    if i > 0 {
                        line(last, start, pt, &mut winding);
                    }
                    start = p;
                    last = p;
                }
                El::Line(p) => {
                    line(last, p, pt, &mut winding);
                    last = p;
                }
                El::Cubic(a, b, c) => {
                    curve([last, a, b, c], pt, &mut winding, 0);
                    last = c;
                }
            }
        }
        if last != start {
            line(last, start, pt, &mut winding);
        }
        if even_odd {
            winding % 2 != 0
        } else {
            winding != 0
        }
    }

    /// QPainterPath::toFillPolygon.
    pub fn to_fill_polygon(&self) -> Vec<D> {
        let mut flats: Vec<Vec<D>> = Vec::new();
        let mut current: Vec<D> = Vec::new();
        let mut last = [0.0, 0.0];
        for e in &self.el {
            match *e {
                El::Move(p) => {
                    if current.len() > 1 {
                        flats.push(std::mem::take(&mut current));
                    }
                    current.clear();
                    current.push(p);
                }
                El::Line(p) => current.push(p),
                El::Cubic(a, b, c) => add_to_polygon([last, a, b, c], &mut current, 0.5),
            }
            last = pt_of(e);
        }
        if current.len() > 1 {
            flats.push(current);
        }
        let mut polygon = Vec::new();
        let Some(first) = flats.first().and_then(|f| f.first().copied()) else {
            return polygon;
        };
        for (i, f) in flats.iter().enumerate() {
            polygon.extend(f.iter().copied());
            if f.first() != f.last() {
                polygon.push(f[0]);
            }
            if i > 0 {
                polygon.push(first);
            }
        }
        polygon
    }
    fn arc_to(&mut self, r: [f64; 4], start: f64, sweep: f64) {
        if r[2] == 0.0 && r[3] == 0.0 {
            return;
        }
        let (s, cv) = curves_for_arc(r, start, sweep);
        self.p_line(s);
        for k in cv.chunks(3) {
            self.p_cubic(k[0], k[1], k[2]);
        }
    }

    pub fn move_to(&mut self, x: f64, y: f64) {
        self.p_move([x, y]);
    }
    pub fn line_to(&mut self, x: f64, y: f64) {
        if self.el.is_empty() {
            self.p_move([x, y]);
        } else if !fcmp_pt(self.cur(), [x, y]) {
            self.p_line([x, y]);
        }
    }
    pub fn bezier_curve_to(&mut self, c1x: f64, c1y: f64, c2x: f64, c2y: f64, x: f64, y: f64) {
        if self.el.is_empty() {
            self.p_move([c1x, c1y]);
        }
        if !fcmp_pt(self.cur(), [x, y]) {
            self.p_cubic([c1x, c1y], [c2x, c2y], [x, y]);
        }
    }
    pub fn arc(&mut self, xc: f64, yc: f64, radius: f64, sar: f64, ear: f64, anticlockwise: bool) {
        if ![xc, yc, sar, ear, radius].iter().all(|v| v.is_finite()) || sar == ear {
            return;
        }
        let (sar, ear, acw) = (-sar, -ear, !anticlockwise);
        let sa = sar.to_degrees() as f32 as f64;
        let ea = ear.to_degrees() as f32 as f64;
        let mut span = 0.0f64;
        let r = [xc - radius, yc - radius, radius * 2.0, radius * 2.0];
        if (!acw && ea - sa >= 360.0) || (acw && sa - ea >= 360.0) {
            span = 360.0;
        } else {
            if !acw && ea < sa {
                span += 360.0;
            } else if acw && sa < ea {
                span -= 360.0;
            }
            if !(fcmp_d(span + (ea - sa) + 1.0, 1.0) && fcmp_d(span.abs(), 360.0)) {
                span += ea - sa;
            }
        }
        if self.el.is_empty() {
            self.p_move(ellipse_coords(r, sa, 0.0).0);
        } else if radius == 0.0 {
            self.p_line([xc, yc]);
            return;
        }
        self.arc_to(r, sa, span);
    }
    /// QPainterPath::addRect (as used by Context2D strokeRect / fillRect paths).
    pub fn add_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        if w == 0.0 && h == 0.0 {
            return;
        }
        self.p_move([x, y]);
        self.el.push(El::Line([x + w, y]));
        self.el.push(El::Line([x + w, y + h]));
        self.el.push(El::Line([x, y + h]));
        self.el.push(El::Line([x, y]));
        self.require_move = true;
    }

    /// The path's elements, for paths built outside (glyph outlines).
    pub fn push_move(&mut self, x: f64, y: f64) {
        self.p_move([x, y]);
    }
    /// QPainterPath::lineTo.
    pub fn push_line(&mut self, x: f64, y: f64) {
        self.p_line([x, y]);
    }
    /// QPainterPath::cubicTo.
    pub fn push_cubic(&mut self, c1: D, c2: D, e: D) {
        self.p_cubic(c1, c2, e);
    }
    pub fn current_position(&self) -> Option<D> {
        self.el.last().map(|e| match *e {
            El::Move(p) | El::Line(p) | El::Cubic(_, _, p) => p,
        })
    }

    /// QPainterPath::map with an affine [a b c d e f] (x' = a x + c y + e).
    pub fn mapped(&self, m: [f64; 6]) -> Path {
        let f = |p: D| {
            [
                m[0] * p[0] + m[2] * p[1] + m[4],
                m[1] * p[0] + m[3] * p[1] + m[5],
            ]
        };
        let mut out = Path::from_el(
            self.el
                .iter()
                .map(|e| match *e {
                    El::Move(p) => El::Move(f(p)),
                    El::Line(p) => El::Line(f(p)),
                    El::Cubic(a, b, c) => El::Cubic(f(a), f(b), f(c)),
                })
                .collect(),
        );
        out.require_move = self.require_move;
        out
    }

    /// QQuickContext2D::arcTo / addArcTo.
    pub fn arc_to_points(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, radius: f64) {
        let (st, end) = ([x1, y1], [x2, y2]);
        let Some(p0) = self.current_position() else {
            self.move_to(x1, y1);
            return;
        };
        if st == p0 || st == end || radius == 0.0 {
            self.line_to(x1, y1);
            return;
        }
        let p1 = st;
        let p1p0 = [p0[0] - p1[0], p0[1] - p1[1]];
        let p1p2 = [end[0] - p1[0], end[1] - p1[1]];
        let l0 = p1p0[0].hypot(p1p0[1]);
        let l2 = p1p2[0].hypot(p1p2[1]);
        let cos_phi = (p1p0[0] * p1p2[0] + p1p0[1] * p1p2[1]) / (l0 * l2);
        let fuzzy_one = |v: f64| (v - 1.0).abs() * 1e12 <= v.abs().min(1.0);
        if fuzzy_one(cos_phi.abs()) {
            self.line_to(p1[0], p1[1]);
            return;
        }
        let tangent = radius / (cos_phi.acos() / 2.0).tan();
        let f0 = tangent / l0;
        let t0 = [p1[0] + f0 * p1p0[0], p1[1] + f0 * p1p0[1]];
        let mut orth = [p1p0[1], -p1p0[0]];
        let ol = orth[0].hypot(orth[1]);
        let fra = radius / ol;
        let cos_alpha = (orth[0] * p1p2[0] + orth[1] * p1p2[1]) / (ol * l2);
        if cos_alpha < 0.0 {
            orth = [-orth[0], -orth[1]];
        }
        let p = [t0[0] + fra * orth[0], t0[1] + fra * orth[1]];
        orth = [-orth[0], -orth[1]];
        let sa = orth[1].atan2(orth[0]);
        let f2 = tangent / l2;
        let t2 = [p1[0] + f2 * p1p2[0], p1[1] + f2 * p1p2[1]];
        let ea = (t2[1] - p[1]).atan2(t2[0] - p[0]);
        let pi = std::f64::consts::PI;
        let acw = (sa > ea && sa - ea < pi) || (sa < ea && ea - sa > pi);
        self.arc(p[0], p[1], radius, sa, ea, acw);
    }

    pub fn close_path(&mut self) {
        if self.el.is_empty() {
            return;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for e in &self.el {
            let pts: Vec<D> = match *e {
                El::Move(p) | El::Line(p) => vec![p],
                El::Cubic(a, b, c) => vec![a, b, c],
            };
            for p in pts {
                x0 = x0.min(p[0]);
                y0 = y0.min(p[1]);
                x1 = x1.max(p[0]);
                y1 = y1.max(p[1]);
            }
        }
        if x1 - x0 != 0.0 || y1 - y0 != 0.0 {
            self.p_close();
        }
    }
}

// ---------------------------------------------------------------- QPathClipper

/// The side of a rectangle a subpath is cut at: the axis, the coordinate, and whether the
/// outside is below it.
#[derive(Clone, Copy)]
struct ClipEdge {
    axis: usize,
    t: f64,
    below: bool,
}

impl ClipEdge {
    fn out(&self, p: D) -> bool {
        if self.below {
            p[self.axis] < self.t
        } else {
            p[self.axis] > self.t
        }
    }
    /// intersectLine: QLineF(a, b).pointAt.
    fn cross(&self, a: D, b: D) -> D {
        let k = self.axis;
        let t = (self.t - a[k]) / (b[k] - a[k]);
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
    }
}

fn bez_at(b: &[D; 4], t: f64) -> D {
    let m = 1.0 - t;
    let one = |k: usize| {
        let a = b[0][k] * m + b[1][k] * t;
        let c = b[1][k] * m + b[2][k] * t;
        let d = b[2][k] * m + b[3][k] * t;
        let a = a * m + c * t;
        let c = c * m + d * t;
        a * m + c * t
    };
    [one(0), one(1)]
}

/// QBezier::parameterSplitLeft: `b` keeps the part after `t`, the part before is returned.
fn bez_split_left(b: &mut [D; 4], t: f64) -> [D; 4] {
    let mut left = [[0.0; 2]; 4];
    for k in 0..2 {
        left[0][k] = b[0][k];
        left[1][k] = b[0][k] + t * (b[1][k] - b[0][k]);
        left[2][k] = b[1][k] + t * (b[2][k] - b[1][k]);
        b[2][k] += t * (b[3][k] - b[2][k]);
        b[1][k] = left[2][k] + t * (b[2][k] - left[2][k]);
        left[2][k] = left[1][k] + t * (left[2][k] - left[1][k]);
        b[0][k] = left[2][k] + t * (b[1][k] - left[2][k]);
        left[3][k] = b[0][k];
    }
    left
}

/// QBezier::getSubRange.
fn bez_sub_range(b: &[D; 4], t0: f64, t1: f64) -> [D; 4] {
    let null = |d: f64| d.abs() <= 1e-12;
    let mut result = if null(t1 - 1.0) {
        *b
    } else {
        let mut temp = *b;
        bez_split_left(&mut temp, t1)
    };
    if !null(t0) {
        bez_split_left(&mut result, t0 / t1);
    }
    result
}

/// QBezier::stationaryYPoints along `k`.
fn bez_stationary(b: &[D; 4], k: usize) -> (usize, f64, f64) {
    let null = |d: f64| d.abs() <= 1e-12;
    let (y1, y2, y3, y4) = (b[0][k], b[1][k], b[2][k], b[3][k]);
    let a = -y1 + 3.0 * y2 - 3.0 * y3 + y4;
    let bq = 2.0 * y1 - 4.0 * y2 + 2.0 * y3;
    let c = -y1 + y2;
    let inside = |t: f64| t > 0.0 && t < 1.0;
    if null(a) {
        if null(bq) {
            return (0, 0.0, 1.0);
        }
        let t0 = -c / bq;
        return (inside(t0) as usize, t0, 1.0);
    }
    let disc = bq * bq - 4.0 * a * c;
    if null(disc) {
        let t0 = -bq / (2.0 * a);
        (inside(t0) as usize, t0, 1.0)
    } else if disc > 0.0 {
        let r = disc.sqrt();
        let (mut t0, mut t1) = ((-bq - r) / (2.0 * a), (-bq + r) / (2.0 * a));
        if t1 < t0 {
            std::mem::swap(&mut t0, &mut t1);
        }
        let mut t = [0.0, 1.0];
        let mut count = 0;
        for v in [t0, t1] {
            if inside(v) {
                t[count] = v;
                count += 1;
            }
        }
        (count, t[0], t[1])
    } else {
        (0, 0.0, 1.0)
    }
}

/// QBezier::tForY along `k`.
fn bez_t_for(b: &[D; 4], k: usize, mut t0: f64, mut t1: f64, y: f64) -> f64 {
    let (mut py0, mut py1) = (bez_at(b, t0)[k], bez_at(b, t1)[k]);
    if py0 > py1 {
        std::mem::swap(&mut py0, &mut py1);
        std::mem::swap(&mut t0, &mut t1);
    }
    if py0 >= y {
        return t0;
    } else if py1 <= y {
        return t1;
    }
    let mut lt = t0;
    loop {
        let t = 0.5 * (t0 + t1);
        let m = 1.0 - t;
        let (mut bb, mut c) = (m * m, t * t);
        let d = c * t;
        let a = bb * m;
        bb *= 3.0 * t;
        c *= 3.0 * m;
        let yt = a * b[0][k] + bb * b[1][k] + c * b[2][k] + d * b[3][k];
        if yt < y {
            t0 = t;
        } else {
            t1 = t;
        }
        let dt = lt - t;
        lt = t;
        if dt.is_nan() || dt.abs() <= 1e-7 {
            break;
        }
    }
    t0
}

impl Path {
    fn join_line(&mut self, a: D, b: D) {
        if self.el.is_empty() {
            self.p_move(a);
        } else {
            self.p_line(a);
        }
        self.p_line(b);
    }

    fn join_bezier(&mut self, b: &[D; 4]) {
        if self.el.is_empty() {
            self.p_move(b[0]);
        } else {
            self.p_line(b[0]);
        }
        self.p_cubic(b[1], b[2], b[3]);
    }

    fn clip_line(&mut self, e: ClipEdge, a: D, b: D) {
        match (e.out(a), e.out(b)) {
            (true, true) => {}
            (true, false) => self.join_line(e.cross(a, b), b),
            (false, true) => self.join_line(a, e.cross(a, b)),
            (false, false) => self.join_line(a, b),
        }
    }

    fn clip_bezier(&mut self, e: ClipEdge, b: [D; 4]) {
        let outs = b.iter().filter(|p| e.out(**p)).count();
        if outs == 4 {
            return;
        }
        if outs == 0 {
            return self.join_bezier(&b);
        }
        let (stationary, t0, t1) = bez_stationary(&b, e.axis);
        let mut segments = [0.0; 4];
        let mut points = [b[0]; 4];
        let mut n = 0;
        for t in [t0, t1].into_iter().take(stationary) {
            n += 1;
            segments[n] = t;
            points[n] = bez_at(&b, t);
        }
        n += 1;
        segments[n] = 1.0;
        points[n] = b[3];
        let mut last = 0.0;
        let mut out_b = false;
        for i in 0..n {
            let out_a = e.out(points[i]);
            out_b = e.out(points[i + 1]);
            if out_a != out_b {
                let x = bez_t_for(&b, e.axis, segments[i], segments[i + 1], e.t);
                if out_b {
                    self.join_bezier(&bez_sub_range(&b, last, x));
                }
                last = x;
            }
        }
        if !out_b {
            self.join_bezier(&bez_sub_range(&b, last, 1.0));
        }
    }

    /// clip<Edge>(path, t) of qpathclipper.cpp: one subpath cut at one side of a rectangle.
    fn clipped_at(&self, e: ClipEdge) -> Path {
        let mut result = Path::default();
        let mut prev = self.el.first().map(pt_of).unwrap_or([0.0; 2]);
        for el in self.el.iter().skip(1) {
            match *el {
                El::Cubic(c1, c2, p) => result.clip_bezier(e, [prev, c1, c2, p]),
                El::Move(p) | El::Line(p) => result.clip_line(e, prev, p),
            }
            prev = pt_of(el);
        }
        let first = self.el.first().map(pt_of).unwrap_or([0.0; 2]);
        if prev != first {
            result.clip_line(e, prev, first);
        }
        result
    }

    /// QPathClipper::pathToRect.
    fn is_clipper_rect(&self) -> bool {
        let [
            El::Move(a),
            El::Line(b),
            El::Line(c),
            El::Line(d),
            El::Line(e),
        ] = self.el[..]
        else {
            return false;
        };
        b[1] == a[1] && c[0] == b[0] && d == [a[0], c[1]] && e == a
    }

    /// QPainterPath::isEmpty.
    fn is_qt_empty(&self) -> bool {
        self.el.is_empty() || matches!(self.el[..], [El::Move(_)])
    }

    /// QPathClipper::intersect(path, rect): `self` with fill rule kept, cut to the rectangle
    /// `r` (x, y, w, h).
    fn intersected_rect(&self, r: [f64; 4]) -> Path {
        let mut subpaths: Vec<Path> = Vec::new();
        let mut current = Path::default();
        for el in &self.el {
            match *el {
                El::Move(p) => {
                    if current.el.len() > 1 {
                        subpaths.push(std::mem::take(&mut current));
                    }
                    current = Path::default();
                    current.p_move(p);
                }
                El::Line(p) => current.p_line(p),
                El::Cubic(c1, c2, p) => current.p_cubic(c1, c2, p),
            }
        }
        if current.el.len() > 1 {
            subpaths.push(current);
        }
        let (left, top, right, bottom) = (r[0], r[1], r[0] + r[2], r[1] + r[3]);
        let mut result = Path::default();
        for mut sub in subpaths {
            let mut b = sub.bounding_rect();
            if !rects_intersect(b, r) {
                continue;
            }
            if b[0] < left {
                sub = sub.clipped_at(ClipEdge {
                    axis: 0,
                    t: left,
                    below: true,
                });
            }
            if b[0] + b[2] > right {
                sub = sub.clipped_at(ClipEdge {
                    axis: 0,
                    t: right,
                    below: false,
                });
            }
            b = sub.bounding_rect();
            if b[1] < top {
                sub = sub.clipped_at(ClipEdge {
                    axis: 1,
                    t: top,
                    below: true,
                });
            }
            if b[1] + b[3] > bottom {
                sub = sub.clipped_at(ClipEdge {
                    axis: 1,
                    t: bottom,
                    below: false,
                });
            }
            if sub.el.len() > 1 {
                result.add_path(&sub);
            }
        }
        let b = result.bounding_rect();
        if b[2] <= 0.0 || b[3] <= 0.0 {
            return Path::default();
        }
        result
    }

    /// QPainterPath::intersected(clip): the area inside both paths. QPathClipper::clip answers
    /// equal paths, paths apart and a rectangle without its edge list. The flags are the
    /// odd-even fill rules; the answer has its own.
    pub fn intersected(&self, even_odd: bool, clip: &Path, clip_even_odd: bool) -> (Path, bool) {
        if self.is_qt_empty() || clip.is_qt_empty() {
            return (Path::default(), true);
        }
        let (sb, cb) = (self.bounding_rect(), clip.bounding_rect());
        if even_odd == clip_even_odd && self.el.len() == clip.el.len() {
            // QPainterPath::operator==.
            let eps = [sb[2] * 1e-12, sb[3] * 1e-12];
            let near = |a: D, b: D| (a[0] - b[0]).abs() <= eps[0] && (a[1] - b[1]).abs() <= eps[1];
            let same = self.el.iter().zip(&clip.el).all(|(a, b)| match (a, b) {
                (El::Move(p), El::Move(q)) | (El::Line(p), El::Line(q)) => near(*p, *q),
                (El::Cubic(a1, a2, a3), El::Cubic(b1, b2, b3)) => {
                    near(*a1, *b1) && near(*a2, *b2) && near(*a3, *b3)
                }
                _ => false,
            });
            if same {
                return (self.clone(), even_odd);
            }
        }
        let (subject_rect, clip_rect) = (self.is_clipper_rect(), clip.is_clipper_rect());
        if !rects_intersect(cb, sb) {
            return (Path::default(), true);
        }
        if rect_contains(cb, sb) {
            if clip_rect {
                return (self.clone(), even_odd);
            }
        } else if rect_contains(sb, cb) && subject_rect {
            return (clip.clone(), clip_even_odd);
        }
        if subject_rect {
            (clip.intersected_rect(sb), clip_even_odd)
        } else if clip_rect {
            (self.intersected_rect(cb), even_odd)
        } else {
            (
                pathclipper::intersected(self, even_odd, clip, clip_even_odd),
                true,
            )
        }
    }

    /// QVectorPath::isRect: the rectangle (x0, y0, x1, y1) a four- or five-point path is.
    pub fn as_rect(&self) -> Option<[f64; 4]> {
        let pts: Vec<D> = match self.el[..] {
            [El::Move(a), El::Line(b), El::Line(c), El::Line(d)] => vec![a, b, c, d],
            [
                El::Move(a),
                El::Line(b),
                El::Line(c),
                El::Line(d),
                El::Line(e),
            ] if e == a => {
                vec![a, b, c, d]
            }
            _ => return None,
        };
        let ok = pts[0][0] == pts[3][0]
            && pts[1][0] == pts[2][0]
            && pts[0][1] == pts[1][1]
            && pts[2][1] == pts[3][1]
            && pts[0][0] < pts[2][0]
            && pts[0][1] < pts[2][1];
        ok.then(|| [pts[0][0], pts[0][1], pts[2][0], pts[2][1]])
    }
}

/// QRectF::intersects for x, y, w, h with w, h >= 0.
fn rects_intersect(a: [f64; 4], b: [f64; 4]) -> bool {
    (0..2).all(|k| {
        let (l1, r1, l2, r2) = (a[k], a[k] + a[k + 2], b[k], b[k] + b[k + 2]);
        l1 != r1 && l2 != r2 && !(l1 >= r2 || l2 >= r1)
    })
}

/// QRectF::contains(QRectF).
fn rect_contains(a: [f64; 4], b: [f64; 4]) -> bool {
    (0..2).all(|k| {
        let (l1, r1, l2, r2) = (a[k], a[k] + a[k + 2], b[k], b[k] + b[k + 2]);
        l1 != r1 && l2 != r2 && !(l2 < l1 || r2 > r1)
    })
}

// ---------------------------------------------------------------- QOutlineMapper

fn bez_split(b: [D; 4]) -> ([D; 4], [D; 4]) {
    let mid = |a: D, b: D| [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    let m12 = mid(b[0], b[1]);
    let m23 = mid(b[1], b[2]);
    let m34 = mid(b[2], b[3]);
    let m1223 = mid(m12, m23);
    let m2334 = mid(m23, m34);
    let m = mid(m1223, m2334);
    ([b[0], m12, m1223, m], [m, m2334, m34, b[3]])
}

/// QBezier::addToPolygon
fn add_to_polygon(b: [D; 4], out: &mut Vec<D>, thr: f64) {
    let mut stack: Vec<([D; 4], i32)> = vec![(b, 9)];
    while let Some(&(b, lvl)) = stack.last() {
        let [p1, p2, p3, p4] = b;
        let (y4y1, x4x1) = (p4[1] - p1[1], p4[0] - p1[0]);
        let mut l = x4x1.abs() + y4y1.abs();
        let d;
        if l > 1.0 {
            d = (x4x1 * (p1[1] - p2[1]) - y4y1 * (p1[0] - p2[0])).abs()
                + (x4x1 * (p1[1] - p3[1]) - y4y1 * (p1[0] - p3[0])).abs();
        } else {
            d = (p1[0] - p2[0]).abs()
                + (p1[1] - p2[1]).abs()
                + (p1[0] - p3[0]).abs()
                + (p1[1] - p3[1]).abs();
            l = 1.0;
        }
        if d < thr * l || lvl == 0 {
            out.push(p4);
            stack.pop();
        } else {
            let (left, right) = bez_split(b);
            let n = stack.len();
            stack[n - 1] = (right, lvl - 1);
            stack.push((left, lvl - 1));
        }
    }
}

fn round26_6(v: f64) -> i64 {
    (v * 64.0 + 0.5).floor() as i64
}

/// Flattened outline: contours of 26.6 points.
/// QOutlineMapper with a painter transform `xf` ([a b c d e f]): curves are flattened in user
/// space with threshold 0.25 / qt_scaleForTransform, then the points are mapped.
pub(crate) fn outline_xf(
    path: &Path,
    dx: f64,
    dy: f64,
    xf: Option<[f64; 6]>,
) -> Vec<Vec<[i64; 2]>> {
    let thr = match xf {
        None => 0.25,
        Some(m) => {
            let scale = scale_for_transform(m).0;
            if scale == 0.0 { 0.25 } else { 0.25 / scale }
        }
    };
    let mut contours: Vec<Vec<D>> = Vec::new();
    let n = path.el.len();
    let close = |c: &mut Vec<Vec<D>>| {
        if let Some(cur) = c.last_mut() {
            let (s, l) = (cur[0], *cur.last().unwrap());
            if cur.len() > 1 && !fcmp_pt(s, l) {
                cur.push(s);
            }
        }
    };
    for (i, e) in path.el.iter().enumerate() {
        match *e {
            El::Move(p) => {
                if i == n - 1 {
                    continue;
                }
                close(&mut contours);
                contours.push(vec![p]);
            }
            El::Line(p) => {
                if contours.is_empty() {
                    contours.push(vec![[0.0, 0.0]]);
                }
                contours.last_mut().unwrap().push(p);
            }
            El::Cubic(c1, c2, ep) => {
                if let Some(cur) = contours.last_mut() {
                    let s = *cur.last().unwrap();
                    add_to_polygon([s, c1, c2, ep], cur, thr);
                }
            }
        }
    }
    close(&mut contours);
    contours
        .into_iter()
        .map(|c| {
            c.into_iter()
                .map(|p| match xf {
                    None => p,
                    Some(m) => [
                        m[0] * p[0] + m[2] * p[1] + m[4],
                        m[1] * p[0] + m[3] * p[1] + m[5],
                    ],
                })
                .map(|p| [round26_6(p[0] + dx), round26_6(p[1] + dy)])
                .collect()
        })
        .collect()
}

// ---------------------------------------------------------------- qgrayraster.c

const PIXEL_BITS: i64 = 8;
const ONE_PIXEL: i64 = 1 << PIXEL_BITS;

fn trunc(x: i64) -> i64 {
    x >> PIXEL_BITS
}
fn fract(x: i64) -> i64 {
    x & (ONE_PIXEL - 1)
}
fn div_mod(a: i64, b: i64) -> (i64, i64) {
    let (mut q, mut r) = (a / b, a % b);
    if r < 0 {
        q -= 1;
        r += b;
    }
    (q, r)
}

#[derive(Clone, Copy)]
struct Cell {
    x: i64,
    cover: i64,
    area: i64,
}

struct Gray {
    ex: i64,
    ey: i64,
    min_ex: i64,
    max_ex: i64,
    min_ey: i64,
    max_ey: i64,
    count_ex: i64,
    count_ey: i64,
    area: i64,
    cover: i64,
    invalid: bool,
    x: i64,
    y: i64,
    rows: Vec<Vec<Cell>>,
}

impl Gray {
    fn record_cell(&mut self) {
        if self.area | self.cover != 0 {
            let x = self.ex.min(self.count_ex);
            let row = &mut self.rows[self.ey as usize];
            let c = match row.binary_search_by_key(&x, |c| c.x) {
                Ok(i) => &mut row[i],
                Err(i) => {
                    row.insert(
                        i,
                        Cell {
                            x,
                            cover: 0,
                            area: 0,
                        },
                    );
                    &mut row[i]
                }
            };
            c.area += self.area;
            c.cover += self.cover;
        }
    }
    fn set_cell(&mut self, mut ex: i64, mut ey: i64) {
        ey -= self.min_ey;
        if ex > self.max_ex {
            ex = self.max_ex;
        }
        ex -= self.min_ex;
        if ex < 0 {
            ex = -1;
        }
        if ex != self.ex || ey != self.ey {
            if !self.invalid {
                self.record_cell();
            }
            self.area = 0;
            self.cover = 0;
            self.ex = ex;
            self.ey = ey;
        }
        self.invalid = (ey as u64 as u32) >= (self.count_ey as u32)
            || ey < 0
            || ey >= self.count_ey
            || ex >= self.count_ex;
    }
    fn start_cell(&mut self, mut ex: i64, ey: i64) {
        if ex > self.max_ex {
            ex = self.max_ex;
        }
        if ex < self.min_ex {
            ex = self.min_ex - 1;
        }
        self.area = 0;
        self.cover = 0;
        self.ex = ex - self.min_ex;
        self.ey = ey - self.min_ey;
        self.invalid = false;
        self.set_cell(ex, ey);
    }
    fn render_scanline(&mut self, ey: i64, x1: i64, mut y1: i64, x2: i64, y2: i64) {
        let mut ex1 = trunc(x1);
        let ex2 = trunc(x2);
        if y1 == y2 {
            self.set_cell(ex2, ey);
            return;
        }
        let mut fx1 = fract(x1);
        let fx2 = fract(x2);
        if ex1 != ex2 {
            let mut dx = x2 - x1;
            let dy = y2 - y1;
            let (p, first, incr);
            if dx > 0 {
                p = (ONE_PIXEL - fx1) * dy;
                first = ONE_PIXEL;
                incr = 1;
            } else {
                p = fx1 * dy;
                first = 0;
                incr = -1;
                dx = -dx;
            }
            let (delta, mut m) = div_mod(p, dx);
            self.area += (fx1 + first) * delta;
            self.cover += delta;
            y1 += delta;
            ex1 += incr;
            self.set_cell(ex1, ey);
            if ex1 != ex2 {
                let (lift, rem) = div_mod(ONE_PIXEL * dy, dx);
                loop {
                    let mut delta = lift;
                    m += rem;
                    if m >= dx {
                        m -= dx;
                        delta += 1;
                    }
                    self.area += ONE_PIXEL * delta;
                    self.cover += delta;
                    y1 += delta;
                    ex1 += incr;
                    self.set_cell(ex1, ey);
                    if ex1 == ex2 {
                        break;
                    }
                }
            }
            fx1 = ONE_PIXEL - first;
        }
        let dy = y2 - y1;
        self.area += (fx1 + fx2) * dy;
        self.cover += dy;
    }
    fn render_line(&mut self, to_x: i64, to_y: i64) {
        let mut ey1 = trunc(self.y);
        let ey2 = trunc(to_y);
        if (ey1 >= self.max_ey && ey2 >= self.max_ey) || (ey1 < self.min_ey && ey2 < self.min_ey) {
            self.x = to_x;
            self.y = to_y;
            return;
        }
        let fy1 = fract(self.y);
        let fy2 = fract(to_y);
        if ey1 == ey2 {
            self.render_scanline(ey1, self.x, fy1, to_x, fy2);
            self.x = to_x;
            self.y = to_y;
            return;
        }
        let dx = to_x - self.x;
        let mut dy = to_y - self.y;
        if dx == 0 {
            let ex = trunc(self.x);
            let two_fx = fract(self.x) << 1;
            let first = if dy > 0 { ONE_PIXEL } else { 0 };
            let mut delta = first - fy1;
            self.area += two_fx * delta;
            self.cover += delta;
            delta = first + first - ONE_PIXEL;
            let area = two_fx * delta;
            let max_ey1 = self.count_ey + self.min_ey;
            if dy < 0 {
                if ey1 > max_ey1 {
                    ey1 = if max_ey1 > ey2 { max_ey1 } else { ey2 };
                } else {
                    ey1 -= 1;
                }
                self.set_cell(ex, ey1);
                while ey1 > ey2 && ey1 >= self.min_ey {
                    self.area += area;
                    self.cover += delta;
                    ey1 -= 1;
                    self.set_cell(ex, ey1);
                }
                if ey1 != ey2 {
                    ey1 = ey2;
                    self.set_cell(ex, ey1);
                }
            } else {
                if ey1 < self.min_ey {
                    ey1 = if self.min_ey < ey2 { self.min_ey } else { ey2 };
                } else {
                    ey1 += 1;
                }
                self.set_cell(ex, ey1);
                while ey1 < ey2 && ey1 < max_ey1 {
                    self.area += area;
                    self.cover += delta;
                    ey1 += 1;
                    self.set_cell(ex, ey1);
                }
                if ey1 != ey2 {
                    ey1 = ey2;
                    self.set_cell(ex, ey1);
                }
            }
            let delta = fy2 - ONE_PIXEL + first;
            self.area += two_fx * delta;
            self.cover += delta;
            self.x = to_x;
            self.y = to_y;
            return;
        }
        let (p, first, incr);
        if dy > 0 {
            p = (ONE_PIXEL - fy1) * dx;
            first = ONE_PIXEL;
            incr = 1;
        } else {
            p = fy1 * dx;
            first = 0;
            incr = -1;
            dy = -dy;
        }
        let (delta, mut m) = div_mod(p, dy);
        let mut x = self.x + delta;
        self.render_scanline(ey1, self.x, fy1, x, first);
        ey1 += incr;
        self.set_cell(trunc(x), ey1);
        if ey1 != ey2 {
            let (lift, rem) = div_mod(ONE_PIXEL * dx, dy);
            loop {
                let mut delta = lift;
                m += rem;
                if m >= dy {
                    m -= dy;
                    delta += 1;
                }
                let x2 = x + delta;
                self.render_scanline(ey1, x, ONE_PIXEL - first, x2, first);
                x = x2;
                ey1 += incr;
                self.set_cell(trunc(x), ey1);
                if ey1 == ey2 {
                    break;
                }
            }
        }
        self.render_scanline(ey1, x, ONE_PIXEL - first, to_x, fy2);
        self.x = to_x;
        self.y = to_y;
    }
    fn move_to(&mut self, p: [i64; 2]) {
        if !self.invalid {
            self.record_cell();
        }
        let (x, y) = (p[0] * (ONE_PIXEL >> 6), p[1] * (ONE_PIXEL >> 6));
        self.start_cell(trunc(x), trunc(y));
        self.x = x;
        self.y = y;
    }
    fn line_to(&mut self, p: [i64; 2]) {
        self.render_line(p[0] * (ONE_PIXEL >> 6), p[1] * (ONE_PIXEL >> 6));
    }
}

/// The farthest outline coordinate rasterized as is, in 26.6 (32767 px). An outline with a
/// point past it is clipped to the canvas first, as QRasterPaintEngine clips an outline that
/// leaves its coordinate range (QOutlineMapper::clipElements): `render_line` walks every
/// scanline an edge crosses, inside the clip or not, so a path 1e9 px tall took 16 s to fill
/// a 60 x 40 canvas.
const COORD_LIMIT: i64 = 32767 << 6;

/// The closed contour `c` clipped to the rectangle `lo`..`hi` (Sutherland-Hodgman): every
/// point inside the rectangle keeps its winding number.
fn clip_contour(c: &[[i64; 2]], lo: [i64; 2], hi: [i64; 2]) -> Vec<[i64; 2]> {
    let mut poly = c.to_vec();
    // (axis, bound, whether the inside is at or above the bound)
    let sides = [
        (0, lo[0], true),
        (0, hi[0], false),
        (1, lo[1], true),
        (1, hi[1], false),
    ];
    for (axis, bound, above) in sides {
        let inside = |p: &[i64; 2]| {
            if above {
                p[axis] >= bound
            } else {
                p[axis] <= bound
            }
        };
        // Where the edge p-q meets the bound, to the nearest 1/64.
        let cross = |p: &[i64; 2], q: &[i64; 2]| {
            let o = 1 - axis;
            let t = (bound as f64 - p[axis] as f64) / (q[axis] as f64 - p[axis] as f64);
            let mut r = [bound; 2];
            r[o] = (p[o] as f64 + (q[o] as f64 - p[o] as f64) * t).round() as i64;
            r
        };
        let mut out = Vec::with_capacity(poly.len() + 4);
        for (k, cur) in poly.iter().enumerate() {
            let prev = &poly[(k + poly.len() - 1) % poly.len()];
            match (inside(prev), inside(cur)) {
                (true, true) => out.push(*cur),
                (false, true) => out.extend([cross(prev, cur), *cur]),
                (true, false) => out.push(cross(prev, cur)),
                (false, false) => {}
            }
        }
        poly = out;
    }
    poly
}

/// `contours` as rasterized into a `clip_w` x `clip_h` clip: clipped to it when a point lies
/// past `COORD_LIMIT`.
fn in_range(
    contours: &[Vec<[i64; 2]>],
    clip_w: i64,
    clip_h: i64,
) -> std::borrow::Cow<'_, [Vec<[i64; 2]>]> {
    let far = |p: &[i64; 2]| p.iter().any(|v| v.abs() > COORD_LIMIT);
    if !contours.iter().flatten().any(far) {
        return contours.into();
    }
    // One pixel around the clip keeps the edges that only touch its border cells.
    let (lo, hi) = ([-64, -64], [(clip_w + 1) << 6, (clip_h + 1) << 6]);
    let clipped: Vec<_> = contours.iter().map(|c| clip_contour(c, lo, hi)).collect();
    clipped.into()
}

/// Rasterize contours, calling span(x, y, len, coverage) like qgrayraster's gray_hline/gray_sweep.
pub(crate) fn gray_render(
    contours: &[Vec<[i64; 2]>],
    even_odd: bool,
    clip_w: i64,
    clip_h: i64,
    mut span: impl FnMut(i64, i64, i64, u8),
) {
    let contours = in_range(contours, clip_w, clip_h);
    let contours = &contours[..];
    let pts: Vec<[i64; 2]> = contours.iter().flatten().copied().collect();
    if pts.is_empty() {
        return;
    }
    let (mut min_ex, mut min_ey, mut max_ex, mut max_ey) =
        (pts[0][0], pts[0][1], pts[0][0], pts[0][1]);
    for p in &pts[1..] {
        min_ex = min_ex.min(p[0]);
        max_ex = max_ex.max(p[0]);
        min_ey = min_ey.min(p[1]);
        max_ey = max_ey.max(p[1]);
    }
    let (mut min_ex, mut min_ey, mut max_ex, mut max_ey) = (
        min_ex >> 6,
        min_ey >> 6,
        (max_ex + 63) >> 6,
        (max_ey + 63) >> 6,
    );
    if max_ex <= 0 || min_ex >= clip_w || max_ey <= 0 || min_ey >= clip_h {
        return;
    }
    min_ex = min_ex.max(0);
    min_ey = min_ey.max(0);
    max_ex = max_ex.min(clip_w);
    max_ey = max_ey.min(clip_h);
    let mut g = Gray {
        ex: 0,
        ey: 0,
        min_ex,
        max_ex,
        min_ey,
        max_ey,
        count_ex: max_ex - min_ex,
        count_ey: max_ey - min_ey,
        area: 0,
        cover: 0,
        invalid: true,
        x: 0,
        y: 0,
        rows: vec![Vec::new(); (max_ey - min_ey) as usize],
    };
    for c in contours {
        if c.is_empty() {
            continue;
        }
        g.move_to(c[0]);
        for p in &c[1..] {
            g.line_to(*p);
        }
        g.line_to(c[0]);
    }
    if !g.invalid {
        g.record_cell();
    }
    let hline =
        |x: i64, y: i64, area: i64, acount: i64, span: &mut dyn FnMut(i64, i64, i64, u8)| {
            let mut cov = (area >> (PIXEL_BITS * 2 + 1 - 8)).abs();
            if even_odd {
                cov &= 511;
                if cov > 256 {
                    cov = 512 - cov;
                } else if cov == 256 {
                    cov = 255;
                }
            } else if cov >= 256 {
                cov = 255;
            }
            if cov != 0 {
                span(x + min_ex, y + min_ey, acount, cov as u8);
            }
        };
    for (yi, row) in g.rows.iter().enumerate() {
        let (mut cover, mut x) = (0i64, 0i64);
        for c in row {
            if c.x > x && cover != 0 {
                hline(x, yi as i64, cover * (ONE_PIXEL * 2), c.x - x, &mut span);
            }
            cover += c.cover;
            let area = cover * (ONE_PIXEL * 2) - c.area;
            if area != 0 && c.x >= 0 {
                hline(c.x, yi as i64, area, 1, &mut span);
            }
            x = c.x + 1;
        }
        if g.count_ex > x && cover != 0 {
            hline(
                x,
                yi as i64,
                cover * (ONE_PIXEL * 2),
                g.count_ex - x,
                &mut span,
            );
        }
    }
}

// ---------------------------------------------------------------- QStroker (qstroker.cpp, qfixed = qreal)

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Cap {
    Flat,
    Square,
    Round,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Join {
    Bevel,
    Miter,
    SvgMiter,
    Round,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Flat,
    Square,
    Miter,
    RoundJoin,
    RoundCap,
    SvgMiter,
}

type L = (D, D);

fn l_dx(l: &L) -> f64 {
    l.1[0] - l.0[0]
}
fn l_dy(l: &L) -> f64 {
    l.1[1] - l.0[1]
}
fn l_len(l: &L) -> f64 {
    l_dx(l).hypot(l_dy(l))
}
fn l_null(l: &L) -> bool {
    fcmp_pt(l.0, l.1)
}
fn l_angle(l: &L) -> f64 {
    let t = (-l_dy(l)).atan2(l_dx(l)).to_degrees();
    let t = if t < 0.0 { t + 360.0 } else { t };
    if fcmp_d(t, 360.0) { 0.0 } else { t }
}
fn l_angle_to(a: &L, b: &L) -> f64 {
    if l_null(a) || l_null(b) {
        return 0.0;
    }
    let d = l_angle(b) - l_angle(a);
    let dn = if d < 0.0 { d + 360.0 } else { d };
    if fcmp_d(d, 360.0) { 0.0 } else { dn }
}
fn l_set_len(l: &mut L, len: f64) {
    let old = l_len(l);
    if old > 0.0 {
        l.1 = [
            l.0[0] + len * (l_dx(l) / old),
            l.0[1] + len * (l_dy(l) / old),
        ];
    }
}
fn l_translate(l: &mut L, dx: f64, dy: f64) {
    l.0 = [l.0[0] + dx, l.0[1] + dy];
    l.1 = [l.1[0] + dx, l.1[1] + dy];
}
#[derive(PartialEq)]
enum Isect {
    No,
    Unbounded,
    Bounded,
}
fn l_intersects(a: &L, b: &L) -> (Isect, D) {
    let av = [a.1[0] - a.0[0], a.1[1] - a.0[1]];
    let bv = [b.0[0] - b.1[0], b.0[1] - b.1[1]];
    let c = [a.0[0] - b.0[0], a.0[1] - b.0[1]];
    let den = av[1] * bv[0] - av[0] * bv[1];
    if den == 0.0 || !den.is_finite() {
        return (Isect::No, [0.0, 0.0]);
    }
    let r = 1.0 / den;
    let na = (bv[1] * c[0] - bv[0] * c[1]) * r;
    let p = [a.0[0] + av[0] * na, a.0[1] + av[1] * na];
    if !(0.0..=1.0).contains(&na) {
        return (Isect::Unbounded, p);
    }
    let nb = (av[0] * c[1] - av[1] * c[0]) * r;
    if !(0.0..=1.0).contains(&nb) {
        return (Isect::Unbounded, p);
    }
    (Isect::Bounded, p)
}

type B = [D; 4];

fn b_point_at(b: &B, t: f64) -> D {
    let m = 1.0 - t;
    std::array::from_fn(|k| {
        let (x1, x2, x3, x4) = (b[0][k], b[1][k], b[2][k], b[3][k]);
        let mut a = x1 * m + x2 * t;
        let mut bb = x2 * m + x3 * t;
        let c = x3 * m + x4 * t;
        a = a * m + bb * t;
        bb = bb * m + c * t;
        a * m + bb * t
    })
}
fn b_normal(b: &B, t: f64) -> D {
    let m = 1.0 - t;
    let (a, bb, c) = (m * m, t * m, t * t);
    let ([x1, y1], [x2, y2], [x3, y3], [x4, y4]) = (b[0], b[1], b[2], b[3]);
    [
        (y2 - y1) * a + (y3 - y2) * bb + (y4 - y3) * c,
        -(x2 - x1) * a - (x3 - x2) * bb - (x4 - x3) * c,
    ]
}
fn b_bounds(b: &B) -> [f64; 4] {
    let (mut x0, mut x1, mut y0, mut y1) = (b[0][0], b[0][0], b[0][1], b[0][1]);
    for p in &b[1..] {
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
    [x0, y0, x1 - x0, y1 - y0]
}
fn b_start_tangent(b: &B) -> L {
    let mut t = (b[0], b[1]);
    if l_null(&t) {
        t = (b[0], b[2]);
    }
    if l_null(&t) {
        t = (b[0], b[3]);
    }
    t
}

#[derive(PartialEq)]
enum Shift {
    Ok,
    Discard,
    Split,
    Circle,
}

fn good_offset(b1: &B, b2: &B, offset: f64, threshold: f64) -> Shift {
    let o2 = offset * offset;
    let max_line = threshold * offset * offset;
    let max_normal = threshold * offset;
    let spacing = 1.0 / 4.0;
    let mut t = spacing;
    for _ in 1..4 {
        let p1 = b_point_at(b1, t);
        let p2 = b_point_at(b2, t);
        let d = (p1[0] - p2[0]) * (p1[0] - p2[0]) + (p1[1] - p2[1]) * (p1[1] - p2[1]);
        if (d - o2).abs() > max_line {
            return Shift::Split;
        }
        let n = b_normal(b1, t);
        let l = n[0].abs() + n[1].abs();
        if l != 0.0 {
            let d = (n[0] * (p1[1] - p2[1]) - n[1] * (p1[0] - p2[0])).abs() / l;
            if d > max_normal {
                return Shift::Split;
            }
        }
        t += spacing;
    }
    Shift::Ok
}

fn shift(o: &B, out: &mut B, offset: f64, threshold: f64) -> Shift {
    let eq = |a: D, b: D| fcmp_d(a[0], b[0]) && fcmp_d(a[1], b[1]);
    let (e12, e23, e34) = (eq(o[0], o[1]), eq(o[1], o[2]), eq(o[2], o[3]));
    let mut pts = [[0.0; 2]; 4];
    let mut map = [0usize; 4];
    let mut np = 0;
    pts[np] = o[0];
    np += 1;
    if !e12 {
        pts[np] = o[1];
        np += 1;
    }
    map[1] = np - 1;
    if !e23 {
        pts[np] = o[2];
        np += 1;
    }
    map[2] = np - 1;
    if !e34 {
        pts[np] = o[3];
        np += 1;
    }
    map[3] = np - 1;
    if np == 1 {
        return Shift::Discard;
    }
    let bb = b_bounds(o);
    if np == 4 && bb[2] < 0.1 * offset && bb[3] < 0.1 * offset {
        let ([x1, y1], [x2, y2], [x3, y3], [x4, y4]) = (o[0], o[1], o[2], o[3]);
        let l = (x1 - x2) * (x1 - x2)
            + (y1 - y2) * (y1 - y2) * (x3 - x4) * (x3 - x4)
            + (y3 - y4) * (y3 - y4);
        let dot = (x1 - x2) * (x3 - x4) + (y1 - y2) * (y3 - y4);
        if dot < 0.0 && dot * dot < 0.8 * l {
            return Shift::Circle;
        }
    }
    let unit_normal = |v: D| {
        let n = [v[1], -v[0]];
        let len = n[0].hypot(n[1]);
        [n[0] / len, n[1] / len]
    };
    let mut sh = [[0.0; 2]; 4];
    let v0 = [pts[1][0] - pts[0][0], pts[1][1] - pts[0][1]];
    if v0[0].hypot(v0[1]) == 0.0 {
        return Shift::Discard;
    }
    let mut pn = unit_normal(v0);
    sh[0] = [pts[0][0] + offset * pn[0], pts[0][1] + offset * pn[1]];
    for i in 1..np - 1 {
        let nn = unit_normal([pts[i + 1][0] - pts[i][0], pts[i + 1][1] - pts[i][1]]);
        let sum = [pn[0] + nn[0], pn[1] + nn[1]];
        let r = 1.0 + pn[0] * nn[0] + pn[1] * nn[1];
        if r.abs() <= 1e-12 {
            sh[i] = [pts[i][0] + offset * pn[0], pts[i][1] + offset * pn[1]];
        } else {
            let k = offset / r;
            sh[i] = [pts[i][0] + k * sum[0], pts[i][1] + k * sum[1]];
        }
        pn = nn;
    }
    sh[np - 1] = [
        pts[np - 1][0] + offset * pn[0],
        pts[np - 1][1] + offset * pn[1],
    ];
    *out = [sh[map[0]], sh[map[1]], sh[map[2]], sh[map[3]]];
    if np > 2 {
        return good_offset(o, out, offset, threshold);
    }
    Shift::Ok
}

fn add_circle(b: &B, offset: f64, o: &mut [B]) -> bool {
    let ([x1, y1], [x2, y2], [x3, y3], [x4, y4]) = (b[0], b[1], b[2], b[3]);
    let mut n = [[y2 - y1, x1 - x2], [0.0; 2], [y4 - y3, x3 - x4]];
    let d0 = n[0][0].hypot(n[0][1]);
    if d0.abs() <= 1e-12 {
        return false;
    }
    n[0] = [n[0][0] / d0, n[0][1] / d0];
    let d2 = n[2][0].hypot(n[2][1]);
    if d2.abs() <= 1e-12 {
        return false;
    }
    n[2] = [n[2][0] / d2, n[2][1] / d2];
    n[1] = [x1 - x2 - x3 + x4, y1 - y2 - y3 + y4];
    let d1 = -(n[1][0] * n[1][0] + n[1][1] * n[1][1]).sqrt();
    n[1] = [n[1][0] / d1, n[1][1] / d1];
    let mut ang = [0.0; 2];
    let mut sign = 1.0;
    for i in 0..2 {
        let c = (n[i][0] * n[i + 1][0] + n[i][1] * n[i + 1][1]).clamp(-1.0, 1.0);
        ang[i] = c.acos() * std::f64::consts::FRAC_1_PI;
    }
    if ang[0] + ang[1] > 1.0 {
        n[1] = [-n[1][0], -n[1][1]];
        ang = [1.0 - ang[0], 1.0 - ang[1]];
        sign = -1.0;
    }
    let circle = [
        [x1 + n[0][0] * offset, y1 + n[0][1] * offset],
        [
            0.5 * (x1 + x4) + n[1][0] * offset,
            0.5 * (y1 + y4) + n[1][1] * offset,
        ],
        [x4 + n[2][0] * offset, y4 + n[2][1] * offset],
    ];
    for i in 0..2 {
        let k = 2.0 * 0.5522847498 * sign * offset * ang[i];
        o[i] = [
            circle[i],
            [circle[i][0] - n[i][1] * k, circle[i][1] + n[i][0] * k],
            [
                circle[i + 1][0] + n[i + 1][1] * k,
                circle[i + 1][1] - n[i + 1][0] * k,
            ],
            circle[i + 1],
        ];
    }
    true
}

/// QBezier::shifted
fn shifted(b: &B, out: &mut [B; 16], offset: f64, mut threshold: f32) -> usize {
    let [p1, p2, p3, p4] = *b;
    if fcmp_d(p1[0], p2[0])
        && fcmp_d(p1[0], p3[0])
        && fcmp_d(p1[0], p4[0])
        && fcmp_d(p1[1], p2[1])
        && fcmp_d(p1[1], p3[1])
        && fcmp_d(p1[1], p4[1])
    {
        return 0;
    }
    let max_seg = 16 - 1;
    let mut st = [[[0.0f64; 2]; 4]; 11];
    let (mut bi, mut o): (isize, usize);
    'redo: loop {
        st[0] = *b;
        bi = 0;
        o = 0;
        while bi >= 0 {
            let stack_segments = bi as usize + 1;
            if stack_segments == 10 || o as isize == max_seg as isize - stack_segments as isize {
                threshold = (threshold as f64 * 1.5) as f32;
                if threshold as f64 > 2.0 {
                    break 'redo;
                }
                continue 'redo;
            }
            let cur = st[bi as usize];
            let res = shift(&cur, &mut out[o], offset, threshold as f64);
            match res {
                Shift::Discard => bi -= 1,
                Shift::Ok => {
                    o += 1;
                    bi -= 1;
                }
                Shift::Circle if max_seg - o >= 2 => {
                    if add_circle(&cur, offset, &mut out[o..]) {
                        o += 2;
                    }
                    bi -= 1;
                }
                _ => {
                    let (l, r) = bez_split(cur);
                    st[bi as usize + 1] = l;
                    st[bi as usize] = r;
                    bi += 1;
                }
            }
        }
        return o;
    }
    while bi >= 0 {
        let cur = st[bi as usize];
        let res = shift(&cur, &mut out[o], offset, threshold as f64);
        if res == Shift::Ok || res == Shift::Split {
            o += 1;
        }
        bi -= 1;
    }
    o
}

#[derive(Clone, Copy, PartialEq)]
enum ET {
    Move,
    Line,
    Curve,
    CurveData,
}

struct Stroker {
    width: f64,
    miter_limit: f64,
    cap: Mode,
    join: Mode,
    curve_threshold: f64,
    back1: D,
    back2: D,
    out: Vec<El>,
}

impl Stroker {
    fn emit_move(&mut self, p: D) {
        self.back2 = self.back1;
        self.back1 = p;
        self.out.push(El::Move(p));
    }
    fn emit_line(&mut self, p: D) {
        self.back2 = self.back1;
        self.back1 = p;
        self.out.push(El::Line(p));
    }
    fn emit_cubic(&mut self, c1: D, c2: D, e: D) {
        if c2 == e {
            if c1 == e {
                self.back2 = self.back1;
            } else {
                self.back2 = c1;
            }
        } else {
            self.back2 = c2;
        }
        self.back1 = e;
        self.out.push(El::Cubic(c1, c2, e));
    }

    fn join_points(&mut self, focal: D, next: L, join: Mode) {
        if fcmp_d(self.back1[0], next.0[0]) && fcmp_d(self.back1[1], next.0[1]) {
            return;
        }
        let prev: L = (self.back2, self.back1);
        let (ty, isect) = l_intersects(&prev, &next);
        let sharp = |ty: &Isect| {
            let short: L = (prev.1, next.0);
            let angle = l_angle_to(&short, &prev);
            *ty == Isect::Bounded || (angle > 90.0 && !fcmp_d(angle, 90.0))
        };
        match join {
            Mode::Flat => {
                if sharp(&ty) {
                    self.emit_line(focal);
                    self.emit_line(next.0);
                    return;
                }
                self.emit_line(next.0);
            }
            Mode::Miter => {
                let lim = self.width * self.miter_limit;
                if sharp(&ty) {
                    self.emit_line(focal);
                    self.emit_line(next.0);
                    return;
                }
                let miter: L = (self.back1, isect);
                if ty == Isect::No || l_len(&miter) > lim {
                    let mut l1 = prev;
                    l_set_len(&mut l1, lim);
                    l_translate(&mut l1, l_dx(&prev), l_dy(&prev));
                    let mut l2 = next;
                    l_set_len(&mut l2, lim);
                    let (dx, dy) = (l_dx(&l2), l_dy(&l2));
                    l_translate(&mut l2, -dx, -dy);
                    self.emit_line(l1.1);
                    self.emit_line(l2.0);
                    self.emit_line(next.0);
                } else {
                    self.emit_line(isect);
                    self.emit_line(next.0);
                }
            }
            Mode::Square => {
                let off = self.width / 2.0;
                let mut l1 = prev;
                let dp = l_dx(&prev) * l_dx(&next) + l_dy(&prev) * l_dy(&next);
                if dp > 0.0 {
                    l1 = (prev.1, prev.0);
                } else {
                    l_translate(&mut l1, l_dx(&prev), l_dy(&prev));
                }
                l_set_len(&mut l1, off);
                let mut l2: L = (next.1, next.0);
                let (dx, dy) = (l_dx(&l2), l_dy(&l2));
                l_translate(&mut l2, dx, dy);
                l_set_len(&mut l2, off);
                self.emit_line(l1.1);
                self.emit_line(l2.1);
                self.emit_line(l2.0);
            }
            Mode::RoundJoin => {
                let off = self.width / 2.0;
                let short: L = (prev.1, next.0);
                let angle = l_angle_to(&short, &prev);
                if (ty == Isect::Bounded || angle > 90.01) && l_len(&next) > off {
                    self.emit_line(focal);
                    self.emit_line(next.0);
                    return;
                }
                let x_axis: L = ([0.0, 0.0], [1.0, 0.0]);
                let a1 = l_angle_to(&x_axis, &prev);
                let a2 = l_angle_to(&x_axis, &next);
                let sweep = (a2 - a1).abs();
                let (_, cv) = curves_for_arc(
                    [focal[0] - off, focal[1] - off, off * 2.0, off * 2.0],
                    a1 + 90.0,
                    -sweep,
                );
                for k in cv.chunks(3) {
                    self.emit_cubic(k[0], k[1], k[2]);
                }
                self.emit_line(next.0);
            }
            Mode::RoundCap => {
                let off = self.width / 2.0;
                let k = 0.5522847498 * off;
                let mut l1 = prev;
                let dp = l_dx(&prev) * l_dx(&next) + l_dy(&prev) * l_dy(&next);
                if dp > 0.0 {
                    l1 = (prev.1, prev.0);
                } else {
                    l_translate(&mut l1, l_dx(&prev), l_dy(&prev));
                }
                l_set_len(&mut l1, k);
                let mut l2: L = (focal, prev.1);
                let (dx, dy) = (l_dx(&l2), l_dy(&l2));
                l_translate(&mut l2, -dy, dx);
                l_set_len(&mut l2, k);
                self.emit_cubic(l1.1, l2.1, l2.0);
                let l2b: L = (l2.0, [l2.0[0] - l_dx(&l2), l2.0[1] - l_dy(&l2)]);
                let (tx, ty2) = (next.0[0] - l1.0[0], next.0[1] - l1.0[1]);
                l_translate(&mut l1, tx, ty2);
                self.emit_cubic(l2b.1, l1.1, l1.0);
            }
            Mode::SvgMiter => {
                if sharp(&ty) {
                    self.emit_line(focal);
                    self.emit_line(next.0);
                    return;
                }
                let miter: L = (focal, isect);
                if ty == Isect::No || l_len(&miter) > self.width * self.miter_limit / 2.0 {
                    self.emit_line(next.0);
                } else {
                    self.emit_line(isect);
                    self.emit_line(next.0);
                }
            }
        }
    }

    /// qt_stroke_side: returns (closed, startTangent)
    fn stroke_side(
        &mut self,
        it: &[(ET, D)],
        cap_first: bool,
        start_tangent: &mut Option<L>,
    ) -> bool {
        let start = it[0].1;
        let mut prev = start;
        let mut first = true;
        let offset = self.width / 2.0;
        let mut i = 1;
        while i < it.len() {
            let (t, p) = it[i];
            i += 1;
            if t == ET::Line {
                let mut line: L = (prev, p);
                if !fcmp_pt(line.0, line.1) {
                    let mut normal: L =
                        (line.0, [line.0[0] + l_dy(&line), line.0[1] - l_dx(&line)]);
                    l_set_len(&mut normal, offset);
                    let (dx, dy) = (l_dx(&normal), l_dy(&normal));
                    l_translate(&mut line, dx, dy);
                    if first {
                        if cap_first {
                            self.join_points(prev, line, self.cap);
                        } else {
                            self.emit_move(line.0);
                        }
                        *start_tangent = Some(line);
                        first = false;
                    } else {
                        self.join_points(prev, line, self.join);
                    }
                    self.emit_line(line.1);
                    prev = p;
                }
            } else if t == ET::Curve {
                let cp2 = it[i].1;
                let ep = it[i + 1].1;
                i += 2;
                let bez: B = [prev, p, cp2, ep];
                let mut oc = [[[0.0; 2]; 4]; 16];
                let count = shifted(&bez, &mut oc, offset, self.curve_threshold as f32);
                if count > 0 {
                    let mut tangent = b_start_tangent(&bez);
                    l_translate(
                        &mut tangent,
                        oc[0][0][0] - bez[0][0],
                        oc[0][0][1] - bez[0][1],
                    );
                    if first {
                        if cap_first {
                            self.join_points(prev, tangent, self.cap);
                        } else {
                            self.emit_move(oc[0][0]);
                        }
                        *start_tangent = Some(tangent);
                        first = false;
                    } else {
                        self.join_points(prev, tangent, self.join);
                    }
                    for c in &oc[..count] {
                        self.emit_cubic(c[1], c[2], c[3]);
                    }
                }
                prev = ep;
            }
        }
        if fcmp_d(start[0], prev[0]) && fcmp_d(start[1], prev[1]) {
            if !first {
                let st = start_tangent.unwrap_or(([0.0; 2], [0.0; 2]));
                self.join_points(prev, st, self.join);
            }
            true
        } else {
            false
        }
    }

    fn process_subpath(&mut self, els: &[(ET, D)]) {
        let fw: Vec<(ET, D)> = els.to_vec();
        let n = els.len();
        let mut bw = Vec::with_capacity(n);
        for pos in (0..n).rev() {
            let mut ce = els[pos];
            if pos == n - 1 {
                ce.0 = ET::Move;
            } else {
                let pe = els[pos + 1].0;
                ce.0 = match pe {
                    ET::Line => ET::Line,
                    ET::CurveData => {
                        if ce.0 == ET::Curve {
                            ET::CurveData
                        } else {
                            ET::Curve
                        }
                    }
                    ET::Curve => ET::CurveData,
                    ET::Move => ce.0,
                };
            }
            bw.push(ce);
        }
        let mut fwt = None;
        let mut bwt = None;
        let fwc = self.stroke_side(&fw, false, &mut fwt);
        let bwc = self.stroke_side(&bw, !fwc, &mut bwt);
        if !bwc
            && let Some(t) = fwt
            && !l_null(&t)
        {
            self.join_points(els[0].1, t, self.cap);
        }
    }
}

/// QPainter::strokePath via QPaintEngineEx::stroke (non-cosmetic pen, solid line): stroke outline path.
pub fn stroke_outline(path: &Path, width: f64, cap: Cap, join: Join, miter_limit: f64) -> Path {
    let mut s = Stroker {
        width,
        miter_limit,
        cap: match cap {
            Cap::Flat => Mode::Flat,
            Cap::Square => Mode::Square,
            Cap::Round => Mode::RoundCap,
        },
        join: match join {
            Join::Bevel => Mode::Flat,
            Join::Miter => Mode::Miter,
            Join::SvgMiter => Mode::SvgMiter,
            Join::Round => Mode::RoundJoin,
        },
        curve_threshold: (1.0 / width).clamp(0.00025, 0.25),
        back1: [0.0; 2],
        back2: [0.0; 2],
        out: Vec::new(),
    };
    let mut els: Vec<(ET, D)> = Vec::new();
    for e in &path.el {
        match *e {
            El::Move(p) => {
                if els.len() > 1 {
                    let cur = std::mem::take(&mut els);
                    s.process_subpath(&cur);
                }
                els.clear();
                els.push((ET::Move, p));
            }
            El::Line(p) => els.push((ET::Line, p)),
            El::Cubic(a, b, c) => {
                els.push((ET::Curve, a));
                els.push((ET::CurveData, b));
                els.push((ET::CurveData, c));
            }
        }
    }
    if els.len() > 1 {
        s.process_subpath(&els);
    }
    Path::from_el(s.out)
}

/// QDashStroker::repetitionLimit
pub(crate) const DASH_REPETITION_LIMIT: f64 = 10000.0;

/// QStrokerOps::setCurveThresholdFromTransform: 0.5 device pixels.
const DASH_THRESHOLD: f64 = 0.5;

/// QTransform::inverted for [a b c d e f]; `None` for a singular matrix.
fn inverted(m: [f64; 6]) -> Option<[f64; 6]> {
    let det = m[0] * m[3] - m[1] * m[2];
    if det == 0.0 {
        return None;
    }
    let (a, b, c, d) = (m[3] / det, -m[1] / det, -m[2] / det, m[0] / det);
    Some([a, b, c, d, -(a * m[4] + c * m[5]), -(b * m[4] + d * m[5])])
}

/// qt_scaleForTransform for [a b c d e f]: the transform's scale factor, and whether it is a
/// uniform scale with or without a rotation (QRasterPaintEngineState::flags.tx_noshear).
fn scale_for_transform(m: [f64; 6]) -> (f64, bool) {
    let fuzzy = |a: f64, b: f64| (a - b).abs() * 1e12 <= a.abs().min(b.abs());
    if m[1] == 0.0 && m[2] == 0.0 {
        let (xs, ys) = (m[0].abs(), m[3].abs());
        return (xs.max(ys), fuzzy(xs, ys));
    }
    // Rotate then scale compares the columns, scale then rotate the rows.
    let (x1, y1) = (m[0] * m[0] + m[2] * m[2], m[1] * m[1] + m[3] * m[3]);
    let (x2, y2) = (m[0] * m[0] + m[1] * m[1], m[2] * m[2] + m[3] * m[3]);
    if (x1 - y1).abs() > (x2 - y2).abs() {
        (x1.max(y1).sqrt(), fuzzy(x1, y1))
    } else {
        (x2.max(y2).sqrt(), fuzzy(x2, y2))
    }
}

/// QPaintEngineEx::stroke: max(width, height) of the control point rect grown by the pen width
/// and clipped to the device.
pub(crate) fn dash_extent(path: &Path, width: f64, clip: [f64; 4]) -> f64 {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for e in &path.el {
        let pts: &[D] = match e {
            El::Move(p) | El::Line(p) => std::slice::from_ref(p),
            El::Cubic(a, b, c) => &[*a, *b, *c],
        };
        for p in pts {
            x0 = x0.min(p[0]);
            y0 = y0.min(p[1]);
            x1 = x1.max(p[0]);
            y1 = y1.max(p[1]);
        }
    }
    let l = (x0 - width).max(clip[0]);
    let t = (y0 - width).max(clip[1]);
    let r = (x1 + width).min(clip[0] + clip[2]);
    let b = (y1 + width).min(clip[1] + clip[3]);
    if l >= r || t >= b {
        return 0.0;
    }
    (r - l).max(b - t)
}

fn line_rect_intersects_rect(p1: D, p2: D, tl: D, br: D) -> bool {
    (p1[0] > tl[0] || p2[0] > tl[0])
        && (p1[0] < br[0] || p2[0] < br[0])
        && (p1[1] > tl[1] || p2[1] > tl[1])
        && (p1[1] < br[1] || p2[1] < br[1])
}

fn line_intersects_rect(mut p1: D, mut p2: D, tl: D, br: D) -> bool {
    if !line_rect_intersects_rect(p1, p2, tl, br) {
        return false;
    }
    if p1[0] == p2[0] || p1[1] == p2[1] {
        return true;
    }
    if p1[1] > p2[1] {
        std::mem::swap(&mut p1, &mut p2);
    }
    let w = [p2[0] - p1[0], p2[1] - p1[1]];
    let (u, v) = if p1[0] < p2[0] {
        (
            [tl[0] - p1[0], br[1] - p1[1]],
            [br[0] - p1[0], tl[1] - p1[1]],
        )
    } else {
        (
            [tl[0] - p1[0], tl[1] - p1[1]],
            [br[0] - p1[0], br[1] - p1[1]],
        )
    };
    let val1 = u[0] * w[1] - u[1] * w[0];
    let val2 = v[0] * w[1] - v[1] * w[0];
    (val1 < 0.0 && val2 > 0.0) || (val1 > 0.0 && val2 < 0.0)
}

/// QDashStroker feeding QStroker: splits every subpath of `path` into its dashes, open
/// subpaths in the same space. `pattern` (even length) and `offset` are in pen-width units
/// (QPen), `clip` is the device rect.
fn dash_path(
    path: &Path,
    width: f64,
    miter_limit: f64,
    pattern: &[f64],
    offset: f64,
    clip: [f64; 4],
    threshold: f64,
) -> Path {
    let mut out = Vec::new();
    let mut sub: Vec<D> = Vec::new();
    let mut last = [0.0; 2];
    for e in &path.el {
        match *e {
            El::Move(p) => {
                if sub.len() > 1 {
                    dash_subpath(&sub, width, miter_limit, pattern, offset, clip, &mut out);
                }
                sub.clear();
                sub.push(p);
            }
            El::Line(p) => sub.push(p),
            // QSubpathFlatIterator: QBezier::toPolygon(m_dashThreshold), start point included.
            El::Cubic(a, b, c) => {
                sub.push(last);
                add_to_polygon([last, a, b, c], &mut sub, threshold);
            }
        }
        last = pt_of(e);
    }
    if sub.len() > 1 {
        dash_subpath(&sub, width, miter_limit, pattern, offset, clip, &mut out);
    }
    Path::from_el(out)
}

/// QDashStroker::processCurrentSubpath over a flattened subpath.
fn dash_subpath(
    pts: &[D],
    width: f64,
    miter_limit: f64,
    pattern: &[f64],
    offset: f64,
    clip: [f64; 4],
    out: &mut Vec<El>,
) {
    let mut dashes = [0.0; 32];
    let mut dash_count = pattern.len().min(32);
    let mut longest = 0.0f64;
    let mut sum = 0.0;
    for (d, p) in dashes.iter_mut().zip(&pattern[..dash_count]) {
        *d = p.max(0.0) * width;
        sum += *d;
        longest = longest.max(*d);
    }
    if sum.abs() <= 1e-12 {
        return;
    }
    let inv_sum = 1.0 / sum;
    dash_count &= !1;

    let mut idash = 0;
    let mut pos = 0.0;
    let mut doffset = (offset * width) % sum;
    if doffset < 0.0 {
        doffset += sum;
    }
    while doffset >= dashes[idash] {
        doffset -= dashes[idash];
        idash += 1;
        if idash >= dash_count {
            idash = 0;
        }
    }

    let finite = |p: D| p[0].is_finite() && p[1].is_finite();
    let mut estart = 0.0;
    let mut prev = pts[0];
    if !finite(prev) {
        return;
    }
    let clipping = clip[2] > 0.0 && clip[3] > 0.0;
    let mut move_to = prev;
    let pad = width.max(miter_limit) * longest;
    let tl = [clip[0] - pad, clip[1] - pad];
    let br = [clip[0] + clip[2] + pad, clip[1] + clip[3] + pad];

    let mut has_move = false;
    for &e in &pts[1..] {
        if !finite(e) {
            continue;
        }
        let (dx, dy) = (e[0] - prev[0], e[1] - prev[1]);
        let mut elen = dx.hypot(dy);
        let estop = estart + elen;
        let mut done = pos >= estop;

        let clip_it = clipping && !line_intersects_rect(prev, e, tl, br);
        let skip_dashing = elen * inv_sum > DASH_REPETITION_LIMIT;
        let mut max_dashes = dash_count as i64;
        if skip_dashing || clip_it {
            elen -= (elen * inv_sum).floor() * sum;
            while !done {
                let dpos = (pos + dashes[idash]) - (doffset + estart);
                if dpos > elen {
                    doffset = dashes[idash] - (dpos - elen);
                    pos = estop;
                    done = true;
                } else {
                    max_dashes -= 1;
                    pos = if max_dashes > 0 { dpos + estart } else { estop };
                    done = pos >= estop;
                    idash += 1;
                    if idash >= dash_count {
                        idash = 0;
                    }
                    doffset = 0.0;
                }
            }
            if clip_it {
                has_move = false;
            } else {
                if !has_move {
                    out.push(El::Move(move_to));
                    has_move = true;
                }
                out.push(El::Line(e));
            }
            move_to = e;
        }

        while !done {
            let has_offset = doffset > 0.0;
            let even = idash & 1 == 0;
            let dpos = (pos + dashes[idash]) - (doffset + estart);
            let p2;
            if dpos > elen {
                doffset = dashes[idash] - (dpos - elen);
                pos = estop;
                done = true;
                p2 = e;
            } else {
                let t = dpos / elen;
                p2 = [prev[0] + dx * t, prev[1] + dy * t];
                pos = dpos + estart;
                done = pos >= estop;
                idash += 1;
                if idash >= dash_count {
                    idash = 0;
                }
                doffset = 0.0;
            }
            if even {
                if !clipping || line_rect_intersects_rect(move_to, p2, tl, br) {
                    // A dash carried over from the previous element continues its subpath.
                    if !has_offset || !has_move {
                        out.push(El::Move(move_to));
                        has_move = true;
                    }
                    out.push(El::Line(p2));
                } else {
                    has_move = false;
                }
            }
            move_to = p2;
        }

        estart = estop;
        prev = e;
    }
}

// ---------------------------------------------------------------- qdrawhelper

/// Canvas paint context: an ARGB32_Premultiplied image (stored as premultiplied RGBA).
pub struct Context {
    pub img: Canvas,
}

impl Context {
    pub fn new(w: usize, h: usize) -> Self {
        Context {
            img: Canvas::new(w, h, [0, 0, 0, 0]),
        }
    }

    /// ctx.stroke() with a solid strokeStyle (pen width > 1).
    pub fn stroke(
        &mut self,
        path: &Path,
        color: [u8; 4],
        width: f64,
        cap: Cap,
        join: Join,
        miter_limit: f64,
    ) {
        self.stroke_dashed(path, color, width, cap, join, miter_limit, &[], 0.0);
    }

    /// ctx.stroke() with optional setLineDash pattern (pen-width units, as QPen keeps it).
    /// Pens of width <= 1 take the raster engine's fast_pen path (QCosmeticStroker); wider
    /// pens go through QDashStroker and QStroker (QPaintEngineEx::stroke).
    #[allow(clippy::too_many_arguments)]
    pub fn stroke_dashed(
        &mut self,
        path: &Path,
        color: [u8; 4],
        width: f64,
        cap: Cap,
        join: Join,
        miter_limit: f64,
        dash: &[f64],
        dash_offset: f64,
    ) {
        // Canvas strokes stay on the stroker they were checked against Qt with.
        self.stroke_impl(
            path,
            color,
            width,
            cap,
            join,
            miter_limit,
            dash,
            dash_offset,
            None,
            false,
            true,
            256,
        );
    }

    /// QPainter::strokePath under the painter transform `xf` ([a b c d e f]), `path` and the pen
    /// `width` in user units. A pen at most one device pixel wide takes the fast_pen path on
    /// the mapped path; a wider one is dashed and outlined in user space and the outline is
    /// filled through the transform, as QPaintEngineEx::stroke does. Separate line segments
    /// under flat or square caps are rasterized one by one (QRasterPaintEngine::stroke,
    /// LinesHint).
    #[allow(clippy::too_many_arguments)]
    pub fn stroke_xf(
        &mut self,
        path: &Path,
        color: [u8; 4],
        width: f64,
        cap: Cap,
        join: Join,
        miter_limit: f64,
        dash: &[f64],
        dash_offset: f64,
        xf: [f64; 6],
    ) {
        self.stroke_xf_opacity(
            path,
            color,
            width,
            cap,
            join,
            miter_limit,
            dash,
            dash_offset,
            xf,
            256,
        );
    }

    /// [`Context::stroke_xf`] under painter opacity `int_opacity` (0..=256,
    /// QPainterState::intOpacity): the pen's colour is qPremultiplyWithExtraAlpha of it.
    #[allow(clippy::too_many_arguments)]
    pub fn stroke_xf_opacity(
        &mut self,
        path: &Path,
        color: [u8; 4],
        width: f64,
        cap: Cap,
        join: Join,
        miter_limit: f64,
        dash: &[f64],
        dash_offset: f64,
        xf: [f64; 6],
        int_opacity: u32,
    ) {
        self.stroke_impl(
            path,
            color,
            width,
            cap,
            join,
            miter_limit,
            dash,
            dash_offset,
            Some(xf),
            true,
            false,
            int_opacity,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn stroke_impl(
        &mut self,
        path: &Path,
        color: [u8; 4],
        width: f64,
        cap: Cap,
        join: Join,
        miter_limit: f64,
        dash: &[f64],
        dash_offset: f64,
        xf: Option<[f64; 6]>,
        lines_hint: bool,
        fast_draw: bool,
        int_opacity: u32,
    ) {
        if path.el.is_empty() || int_opacity == 0 {
            return;
        }
        let (scale, noshear) = xf.map_or((1.0, true), scale_for_transform);
        if noshear && width * scale <= 1.0 {
            let mapped;
            let dev = match xf {
                Some(m) => {
                    mapped = path.mapped(m);
                    &mapped
                }
                None => path,
            };
            crate::cosmetic::stroke(
                &mut self.img,
                dev,
                if fast_draw {
                    crate::cosmetic::Pen::Fast(color)
                } else {
                    crate::cosmetic::Pen::Spans(solid_pm(color, int_opacity))
                },
                width * scale,
                cap != Cap::Flat,
                dash,
                dash_offset,
            );
            return;
        }
        // QPen::setDashPattern pads an odd pattern with 1.
        let mut pattern = dash.to_vec();
        if pattern.len() % 2 == 1 {
            pattern.push(1.0);
        }
        // QVectorPathConverter: a path of alternating moveTo and lineTo has LinesHint;
        // non_complex_pen is a flat or square cap under a transform without shear.
        let is_lines =
            path.el.iter().enumerate().all(|(i, e)| {
                matches!(e, El::Line(_)) == (i % 2 == 1) && !matches!(e, El::Cubic(..))
            });
        if lines_hint && noshear && cap != Cap::Round && is_lines {
            let dev = path.mapped(xf.unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]));
            stroke_lines(
                &mut self.img,
                solid_pm(color, int_opacity),
                &dev,
                width * scale,
                cap == Cap::Square,
                &pattern,
                dash_offset,
            );
            return;
        }
        let mut color = color;
        let mut dashed = None;
        if !dash.is_empty() {
            // QPaintEngineEx::stroke: no pen for an empty pattern; a pattern repeating more than
            // QDashStroker::repetitionLimit times over the visible extent becomes a solid line
            // at half alpha. The device rect is taken to user space.
            let (w, h) = (self.img.w as f64, self.img.h as f64);
            let clip = match xf.and_then(inverted) {
                Some(inv) => {
                    let c = [[0.0, 0.0], [w, 0.0], [0.0, h], [w, h]].map(|p| {
                        [
                            inv[0] * p[0] + inv[2] * p[1] + inv[4],
                            inv[1] * p[0] + inv[3] * p[1] + inv[5],
                        ]
                    });
                    let x0 = c.iter().map(|p| p[0]).fold(f64::MAX, f64::min);
                    let y0 = c.iter().map(|p| p[1]).fold(f64::MAX, f64::min);
                    let x1 = c.iter().map(|p| p[0]).fold(f64::MIN, f64::max);
                    let y1 = c.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
                    [x0, y0, x1 - x0, y1 - y0]
                }
                None => [0.0, 0.0, w, h],
            };
            let pattern_length = pattern.iter().take(32).map(|v| v.max(0.0)).sum::<f64>() * width;
            if pattern_length.abs() <= 1e-12 {
                return;
            }
            if dash_extent(path, width, clip) / pattern_length > DASH_REPETITION_LIMIT {
                color[3] /= 2;
            } else {
                let threshold = if scale == 0.0 {
                    DASH_THRESHOLD
                } else {
                    DASH_THRESHOLD / scale
                };
                dashed = Some(dash_path(
                    path,
                    width,
                    miter_limit,
                    &pattern,
                    dash_offset,
                    clip,
                    threshold,
                ));
            }
        }
        let path = dashed.as_ref().unwrap_or(path);
        if path.el.is_empty() {
            return;
        }
        let o = stroke_outline(path, width, cap, join, miter_limit);
        self.fill_pm(&o, solid_pm(color, int_opacity), false, xf);
    }

    /// ctx.fill() with a solid fillStyle (straight RGBA): closeSubpath + QPainter::fillPath, WindingFill.
    pub fn fill(&mut self, path: &Path, color: [u8; 4], even_odd: bool) {
        if path.el.is_empty() {
            return;
        }
        let mut p = path.clone();
        p.p_close();
        self.fill_raw(&p, color, even_odd);
    }

    /// QPainter::fillPath with a solid brush and painter opacity `int_opacity` (0..=256,
    /// QPainterState::intOpacity): qPremultiplyWithExtraAlpha in 16-bit, then toArgb32.
    pub fn fill_opacity(
        &mut self,
        path: &Path,
        color: [u8; 4],
        even_odd: bool,
        int_opacity: u32,
        xf: Option<[f64; 6]>,
    ) {
        self.fill_pm(path, solid_pm(color, int_opacity), even_odd, xf);
    }

    fn fill_raw(&mut self, p: &Path, color: [u8; 4], even_odd: bool) {
        self.fill_pm(p, raw_pm(color), even_odd, None);
    }

    fn fill_pm(&mut self, p: &Path, pm: [u32; 4], even_odd: bool, xf: Option<[f64; 6]>) {
        if p.el.is_empty() || pm[3] == 0 {
            return;
        }
        let (w, h) = (self.img.w, self.img.h);
        let img = &mut self.img;
        gray_render(
            &outline_xf(p, 0.0, 0.0, xf),
            even_odd,
            w as i64,
            h as i64,
            |x, y, len, cov| blend_span(img, pm, x, y, len, cov as u32),
        );
    }

    /// QPainter::drawRect (brush only, Antialiasing) under a scale/translate transform:
    /// QRasterPaintEngine::fill RectangleHint -> QRasterizer::rasterizeLine(a, b, h / w).
    pub fn fill_rect_xf(&mut self, r: [f64; 4], color: [u8; 4], int_opacity: u32, m: [f64; 6]) {
        let pm = solid_pm(color, int_opacity);
        if pm[3] == 0 {
            return;
        }
        if (m[1] != 0.0 || m[2] != 0.0) && no_shear(m) {
            let [x, y, rw, rh] = normalized(r);
            if rw == 0.0 || rh == 0.0 {
                return;
            }
            let mp =
                |px: f64, py: f64| [m[0] * px + m[2] * py + m[4], m[1] * px + m[3] * py + m[5]];
            let (a, b) = (mp(x, y + rh * 0.5), mp(x + rw, y + rh * 0.5));
            return rasterize_line(&mut self.img, pm, a, b, rh / rw, false);
        }
        let (w, h) = (self.img.w, self.img.h);
        let img = &mut self.img;
        let done = rect_spans(r, m, w, h, &mut |x, y, len, cov| {
            blend_span(img, pm, x, y, len, cov)
        });
        if !done {
            let mut p = Path::default();
            let n = normalized(r);
            p.add_rect(n[0], n[1], n[2], n[3]);
            self.fill_pm(&p, pm, false, Some(m));
        }
    }
}

/// Straight RGBA premultiplied as the raster engine's solid pen colour.
fn raw_pm(color: [u8; 4]) -> [u32; 4] {
    let a = color[3] as u32;
    [
        byte_mul(color[0] as u32, a),
        byte_mul(color[1] as u32, a),
        byte_mul(color[2] as u32, a),
        a,
    ]
}

/// QRasterPaintEngine::stroke for a LinesHint path: every segment of `dev` (device space) is
/// its own line of `width` device pixels, dashed from the start of the pattern (pen widths).
fn stroke_lines(
    img: &mut Canvas,
    pm: [u32; 4],
    dev: &Path,
    width: f64,
    square_cap: bool,
    pattern: &[f64],
    dash_offset: f64,
) {
    let mut dash_index = 0;
    let mut dash_offset = dash_offset;
    let mut in_dash = true;
    let pattern_length: f64 = pattern.iter().sum();
    if pattern_length > 0.0 {
        dash_offset %= pattern_length;
        if dash_offset < 0.0 {
            dash_offset += pattern_length;
        }
        while dash_offset >= pattern[dash_index] {
            dash_offset -= pattern[dash_index];
            dash_index = (dash_index + 1) % pattern.len();
            in_dash = !in_dash;
        }
    }
    for [from, to] in dev.el.as_chunks::<2>().0 {
        let (p1, p2) = (pt_of(from), pt_of(to));
        if p1 == p2 {
            if square_cap {
                let d = width / 2.0;
                rasterize_line(img, pm, [p1[0] - d, p1[1]], [p1[0] + d, p1[1]], 1.0, false);
            }
            continue;
        }
        let length = (p2[0] - p1[0]).hypot(p2[1] - p1[1]);
        if pattern.is_empty() {
            rasterize_line(img, pm, p1, p2, width / length, square_cap);
            continue;
        }
        // QRasterPaintEnginePrivate::rasterizeLine_dashed, restarted on every line.
        if pattern_length <= 0.0 {
            continue;
        }
        if length / (pattern_length * width) > DASH_REPETITION_LIMIT {
            rasterize_line(img, pm, p1, p2, width / length, square_cap);
            continue;
        }
        let (mut index, mut offset, mut on) = (dash_index, dash_offset, in_dash);
        let (mut start, mut left) = (p1, length);
        while left > 0.0 {
            let rasterize = on;
            let mut dash = (pattern[index] - offset) * width;
            let mut end = p2;
            // QLineF::length of the remaining line, as the source recomputes it.
            let rest = (p2[0] - start[0]).hypot(p2[1] - start[1]);
            if dash >= left {
                dash = rest;
                offset += dash / width;
                left = 0.0;
            } else {
                offset = 0.0;
                on = !on;
                index = (index + 1) % pattern.len();
                left -= dash;
                // QLineF::setLength
                let (dx, dy) = (p2[0] - start[0], p2[1] - start[1]);
                end = [start[0] + dash * (dx / rest), start[1] + dash * (dy / rest)];
            }
            if rasterize && dash > 0.0 {
                rasterize_line(img, pm, start, end, width / dash, square_cap);
            }
            start = end;
        }
    }
}

/// QScFixed of qrasterizer.cpp: 16.16 fixed point.
const SC: i64 = 65536;

/// qSafeFloatToQScFixed
fn sc(v: f64) -> i64 {
    let t = v * SC as f64;
    if t > i32::MAX as f64 * SC as f64 {
        i32::MAX as i64 * SC
    } else if t < i32::MIN as f64 * SC as f64 {
        i32::MIN as i64 * SC
    } else {
        t as i64
    }
}

/// QScFixedMultiply and QScFixedFastMultiply
fn sc_mul(x: i64, y: i64) -> i64 {
    x.wrapping_mul(y) >> 16
}

/// qSafeDivide
fn safe_div(x: f64, y: f64) -> f64 {
    if y == 0.0 {
        if x > 0.0 { 1e20 } else { -1e20 }
    } else {
        x / y
    }
}

/// intersectPixelFP of qrasterizer.cpp: the part of pixel column `x` between `top` and `bottom`
/// that lies left of an edge crossing those rows at `li` and `ri`.
fn intersect_pixel_fp(
    x: i64,
    top: i64,
    bottom: i64,
    li: i64,
    ri: i64,
    slope: i64,
    inv: i64,
) -> i64 {
    let left_x = x * SC;
    let right_x = left_x + SC;
    let ys = || {
        let ly = top + sc_mul(left_x - if slope > 0 { li } else { ri }, inv);
        (ly, ly + inv)
    };
    if li >= left_x && ri <= right_x {
        sc_mul(bottom - top, li - left_x + ((ri - li) >> 1))
    } else if li >= right_x {
        bottom - top
    } else if li >= left_x {
        let (_, ry) = ys();
        if slope > 0 {
            (bottom - top) - sc_mul((right_x - li) >> 1, ry - top)
        } else {
            (bottom - top) - sc_mul((right_x - li) >> 1, bottom - ry)
        }
    } else if ri <= left_x {
        0
    } else if ri <= right_x {
        let (ly, _) = ys();
        if slope > 0 {
            sc_mul((ri - left_x) >> 1, bottom - ly)
        } else {
            sc_mul((ri - left_x) >> 1, ly - top)
        }
    } else {
        let (ly, ry) = ys();
        if slope > 0 {
            (bottom - ry) + ((ry - ly) >> 1)
        } else {
            (ry - top) + ((ly - ry) >> 1)
        }
    }
}

/// qClipLine of qrasterizer.cpp; true when the whole line is clipped away.
fn clip_line(p1: &mut D, p2: &mut D, [xmin, ymin, xmax, ymax]: [f64; 4]) -> bool {
    if !(p1[0].is_finite() && p1[1].is_finite() && p2[0].is_finite() && p2[1].is_finite()) {
        return true;
    }
    let (mut x1, mut y1, mut x2, mut y2) = (p1[0], p1[1], p2[0], p2[1]);
    if x1 < xmin {
        if x2 <= xmin {
            return true;
        }
        y1 += (y2 - y1) / (x2 - x1) * (xmin - x1);
        x1 = xmin;
    } else if x1 > xmax {
        if x2 >= xmax {
            return true;
        }
        y1 += (y2 - y1) / (x2 - x1) * (xmax - x1);
        x1 = xmax;
    }
    if x2 < xmin {
        y2 += (y2 - y1) / (x2 - x1) * (xmin - x2);
        x2 = xmin;
    } else if x2 > xmax {
        y2 += (y2 - y1) / (x2 - x1) * (xmax - x2);
        x2 = xmax;
    }
    if y1 < ymin {
        if y2 <= ymin {
            return true;
        }
        x1 += (x2 - x1) / (y2 - y1) * (ymin - y1);
        y1 = ymin;
    } else if y1 > ymax {
        if y2 >= ymax {
            return true;
        }
        x1 += (x2 - x1) / (y2 - y1) * (ymax - y1);
        y1 = ymax;
    }
    if y2 < ymin {
        x2 += (x2 - x1) / (y2 - y1) * (ymin - y2);
        y2 = ymin;
    } else if y2 > ymax {
        x2 += (x2 - x1) / (y2 - y1) * (ymax - y2);
        y2 = ymax;
    }
    (*p1, *p2) = ([x1, y1], [x2, y2]);
    false
}

/// QRasterizer::rasterizeLine before its spans: the ends of the line from `a` to `b` cut to a
/// device of `cw` by `ch` pixels and `width`, relative to its length, of the cut line. `None`
/// where the line draws nothing.
fn line_ends(
    a: D,
    b: D,
    mut width: f64,
    square_cap: bool,
    cw: i64,
    ch: i64,
) -> Option<(D, D, f64)> {
    if a == b || width.is_nan() || width <= 0.0 || cw == 0 || ch == 0 {
        return None;
    }
    let (mut pa, mut pb) = (a, b);
    if square_cap {
        let d = [pb[0] - pa[0], pb[1] - pa[1]];
        pa = [pa[0] - 0.5 * width * d[0], pa[1] - 0.5 * width * d[1]];
        pb = [pb[0] + 0.5 * width * d[0], pb[1] + 0.5 * width * d[1]];
    }
    let offs = [
        (b[1] - a[1]).abs() * width * 0.5,
        (b[0] - a[0]).abs() * width * 0.5,
    ];
    // A QRectF keeps its width and height, so its right and bottom are sums.
    let (left, top) = (-offs[0], -offs[1]);
    let clip = [
        left,
        top,
        left + ((cw as f64 + offs[0]) - left),
        top + ((ch as f64 + offs[1]) - top),
    ];
    if clip_line(&mut pa, &mut pb, clip) {
        return None;
    }
    // The width is relative to |b - a|.
    let w0 = (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2);
    let w = (pa[0] - pb[0]).powi(2) + (pa[1] - pb[1]).powi(2);
    if w == 0.0 {
        return None;
    }
    width *= (w0 / w).sqrt();
    Some((pa, pb, width))
}

/// q26Dot6Compare
fn q26(p1: f64, p2: f64) -> bool {
    ((p2 - p1) * 64.0) as i64 == 0
}

/// QRasterizer::rasterizeLine, not antialiased (what an image drawn turned is cut by): the
/// line from `a` to `b`, `width` relative to its length, as `span(x, length, y)` of whole
/// pixels inside a device of `cw` by `ch`.
pub(crate) fn aliased_line(
    a: D,
    b: D,
    width: f64,
    cw: i64,
    ch: i64,
    mut span: impl FnMut(i64, i64, i64),
) {
    let Some((mut pa, mut pb, mut width)) = line_ends(a, b, width, false, cw, ch) else {
        return;
    };
    // int(v - 0.5f), -1 below the first pixel's centre.
    let last = |v: f64| if v < 0.5 { -1 } else { (v - 0.5) as i64 };
    if q26(pa[1], pb[1]) {
        let x = (pa[0] + pb[0]) * 0.5;
        let dx = (pb[0] - pa[0]).abs() * 0.5;
        let dy = width * dx;
        (pa, pb) = ([x, pa[1] - dy], [x, pa[1] + dy]);
        width = 1.0 / width;
    }
    if q26(pa[0], pb[0]) {
        if pa[1] > pb[1] {
            std::mem::swap(&mut pa, &mut pb);
        }
        let half = 0.5 * width * (pb[1] - pa[1]);
        let left = (pa[0] - half).clamp(0.0, cw as f64);
        let right = (pa[0] + half).clamp(0.0, cw as f64);
        pa[1] = pa[1].clamp(0.0, ch as f64);
        pb[1] = pb[1].clamp(0.0, ch as f64);
        if q26(left, right) || q26(pa[1], pb[1]) {
            return;
        }
        let (i_left, i_right) = ((left + 0.5) as i64, last(right));
        for y in (pa[1] + 0.5) as i64..=last(pb[1]) {
            if i_right >= i_left {
                span(i_left, i_right - i_left + 1, y);
            }
        }
        return;
    }
    if pa[1] > pb[1] {
        std::mem::swap(&mut pa, &mut pb);
    }
    let delta = [(pb[0] - pa[0]) * 0.5 * width, (pb[1] - pa[1]) * 0.5 * width];
    let perp = [delta[1], -delta[0]];
    let add = |p: D, q: D| [p[0] + q[0], p[1] + q[1]];
    let sub = |p: D, q: D| [p[0] - q[0], p[1] - q[1]];
    let (top, left, right, bottom) = if pa[0] < pb[0] {
        (add(pa, perp), sub(pa, perp), add(pb, perp), sub(pb, perp))
    } else {
        (sub(pa, perp), sub(pb, perp), add(pa, perp), add(pb, perp))
    };
    // snapTo26Dot6Grid
    let snap = |p: D| [(p[0] * 64.0).floor() / 64.0, (p[1] * 64.0).floor() / 64.0];
    let (top, left, right, bottom) = (snap(top), snap(left), snap(right), snap(bottom));
    let slope = |from: D, to: D| safe_div(to[0] - from[0], to[1] - from[1]);
    let (tl, bl) = (slope(top, left), slope(left, bottom));
    let (tr, br) = (slope(top, right), slope(right, bottom));

    let i_top = (top[1] + 0.5) as i64;
    let (i_left, i_right, i_bottom) = (last(left[1]), last(right[1]), last(bottom[1]));
    // The left and the right edge at a row's centre: above the corner (A) and below it (B).
    let at = |p: D, half: f64, row: f64, slope: f64| sc(p[0] + half + (row - p[1]) * slope);
    let mut left_a = at(top, 0.5, i_top as f64 + 0.5, tl);
    let mut left_b = at(left, 0.5, i_left as f64 + 1.5, bl);
    let mut right_a = at(top, -0.5, i_top as f64 + 0.5, tr);
    let mut right_b = at(right, -0.5, i_right as f64 + 1.5, br);
    let mut y = i_top;
    // DO_SEGMENT: the rows down to `next`, between the edges `li` and `ri`.
    let mut segment = |next: i64, li: &mut i64, ri: &mut i64, ls: i64, rs: i64| {
        let ny = (next + 1).min(0);
        if y < ny {
            *li += ls * (ny - y);
            *ri += rs * (ny - y);
            y = ny;
        }
        while y <= next.min(ch - 1) {
            let (x1, x2) = ((*li >> 16).max(0), (*ri >> 16).min(cw - 1));
            if x2 >= x1 {
                span(x1, x2 - x1 + 1, y);
            }
            *li += ls;
            *ri += rs;
            y += 1;
        }
    };
    let (tl, bl, tr, br) = (sc(tl), sc(bl), sc(tr), sc(br));
    segment(i_left.min(i_right), &mut left_a, &mut right_a, tl, tr);
    segment(i_right, &mut left_b, &mut right_a, bl, tr);
    segment(i_left, &mut left_a, &mut right_b, tl, br);
    segment(i_bottom, &mut left_b, &mut right_b, bl, br);
}

/// QRasterizer::rasterizeLine, antialiased: the line from `a` to `b` (device space), `width`
/// relative to its length, as a rectangle with its own pixel coverage.
fn rasterize_line(img: &mut Canvas, pm: [u32; 4], a: D, b: D, width: f64, square_cap: bool) {
    let (cw, ch) = (img.w as i64, img.h as i64);
    let Some((mut pa, mut pb, mut width)) = line_ends(a, b, width, square_cap, cw, ch) else {
        return;
    };

    let mut span = |x: i64, len: i64, y: i64, cov: i64| {
        if cov > 0 && len > 0 {
            blend_span(img, pm, x, y, len, cov.min(255) as u32);
        }
    };
    if q26(pa[1], pb[1]) {
        let x = (pa[0] + pb[0]) * 0.5;
        let dx = (pb[0] - pa[0]).abs() * 0.5;
        let dy = width * dx;
        (pa, pb) = ([x, pa[1] - dy], [x, pa[1] + dy]);
        width = 1.0 / width;
    }
    if q26(pa[0], pb[0]) {
        if pa[1] > pb[1] {
            std::mem::swap(&mut pa, &mut pb);
        }
        let half = 0.5 * width * (pb[1] - pa[1]);
        let left = (pa[0] - half).clamp(0.0, cw as f64);
        let right = (pa[0] + half).clamp(0.0, cw as f64);
        pa[1] = pa[1].clamp(0.0, ch as f64);
        pb[1] = pb[1].clamp(0.0, ch as f64);
        if q26(left, right) || q26(pa[1], pb[1]) {
            return;
        }
        let (il, ir) = (left as i64, right as i64);
        let lw = (il + 1) * SC - sc(left);
        let rw = sc(right) - ir * SC;
        let mut spans: Vec<(i64, i64, i64)> = Vec::with_capacity(3);
        if il == ir {
            let c = if lw == SC { rw } else { rw + lw - SC };
            spans.push((il, 1, c * 255));
        } else {
            if lw == SC {
                spans.push((il, ir - il, lw * 255));
            } else {
                spans.push((il, 1, lw * 255));
                if ir - il > 1 {
                    spans.push((il + 1, ir - il - 1, 255 * SC));
                }
            }
            if rw != 0 {
                spans.push((ir, 1, rw * 255));
            }
        }
        let (itop, ibot) = ((pa[1] as i64) * SC, (pb[1] as i64) * SC);
        let (ya, yb) = (sc(pa[1]), sc(pb[1]));
        let mut yf = itop;
        while yf <= ibot {
            let row_h = (yf + SC).min(yb) - yf.max(ya);
            let y = yf >> 16;
            if y > ch - 1 {
                break;
            }
            for &(x, len, c) in &spans {
                span(x, len, y, sc_mul(row_h, c) >> 16);
            }
            yf += SC;
        }
        return;
    }

    if pa[1] > pb[1] {
        std::mem::swap(&mut pa, &mut pb);
    }
    let delta = [(pb[0] - pa[0]) * 0.5 * width, (pb[1] - pa[1]) * 0.5 * width];
    let perp = [delta[1], -delta[0]];
    let add = |p: D, q: D| [p[0] + q[0], p[1] + q[1]];
    let sub = |p: D, q: D| [p[0] - q[0], p[1] - q[1]];
    let (top, left, right, bottom) = if pa[0] < pb[0] {
        (add(pa, perp), sub(pa, perp), add(pb, perp), sub(pb, perp))
    } else {
        (sub(pa, perp), sub(pb, perp), add(pa, perp), add(pb, perp))
    };
    // snapTo26Dot6Grid
    let snap = |p: D| [(p[0] * 64.0).floor() / 64.0, (p[1] * 64.0).floor() / 64.0];
    let (top, left, right, bottom) = (snap(top), snap(left), snap(right), snap(bottom));

    let top_bound = top[1].clamp(0.0, (ch - 1) as f64);
    let bottom_bound = bottom[1].clamp(0.0, (ch - 1) as f64);
    let slope = |from: D, to: D| safe_div(to[0] - from[0], to[1] - from[1]);
    let tl_slope = slope(top, left);
    let bl_slope = slope(left, bottom);
    let tr_slope = slope(top, right);
    let br_slope = slope(right, bottom);
    let (tl_fp, tr_fp, bl_fp, br_fp) = (sc(tl_slope), sc(tr_slope), sc(bl_slope), sc(br_slope));
    let inv_tl = sc(safe_div(1.0, tl_slope));
    let inv_tr = sc(safe_div(1.0, tr_slope));
    let inv_bl = sc(safe_div(1.0, bl_slope));
    let inv_br = sc(safe_div(1.0, br_slope));

    let int_top = top_bound as i64;
    let i_top = int_top * SC;
    let i_left = (left[1] as i64) * SC;
    let i_right = (right[1] as i64) * SC;
    let i_bottom = (bottom_bound as i64) * SC;

    let mut left_a = sc(top[0] + (int_top as f64 - top[1]) * tl_slope);
    let mut right_a = sc(top[0] + (int_top as f64 - top[1]) * tr_slope);
    let mut left_b = 0;
    let mut right_b = 0;
    if i_left < i_top {
        left_b = sc(left[0] + (int_top as f64 - left[1]) * bl_slope);
    }
    if i_right < i_top {
        right_b = sc(right[0] + (int_top as f64 - right[1]) * br_slope);
    }
    let (y_top, y_left, y_right, y_bottom) = (sc(top[1]), sc(left[1]), sc(right[1]), sc(bottom[1]));

    let mut row_top = i_top.max(y_top);
    let mut top_left_a = left_a + sc_mul(tl_fp, row_top - i_top);
    let mut top_right_a = right_a + sc_mul(tr_fp, row_top - i_top);
    let bound = |v: i64| v.clamp(0, cw - 1);

    let mut yf = i_top;
    while yf <= i_bottom {
        let row_bottom_left = (yf + SC).min(y_left);
        let row_bottom_right = (yf + SC).min(y_right);
        let row_top_left = yf.max(y_left);
        let row_top_right = yf.max(y_right);
        let row_bottom = (yf + SC).min(y_bottom);
        let y = yf >> 16;

        let (top_left_b, bottom_left_a);
        if yf == i_left {
            left_b = sc(left[0] + (y as f64 - left[1]) * bl_slope);
            top_left_b = left_b + sc_mul(bl_fp, row_top_left - yf);
            bottom_left_a = left_a + sc_mul(tl_fp, row_bottom_left - yf);
        } else {
            top_left_b = left_b;
            bottom_left_a = left_a + tl_fp;
        }
        let (top_right_b, bottom_right_a);
        if yf == i_right {
            right_b = sc(right[0] + (y as f64 - right[1]) * br_slope);
            top_right_b = right_b + sc_mul(br_fp, row_top_right - yf);
            bottom_right_a = right_a + sc_mul(tr_fp, row_bottom_right - yf);
        } else {
            top_right_b = right_b;
            bottom_right_a = right_a + tr_fp;
        }
        let (bottom_left_b, bottom_right_b) = if yf == i_bottom {
            (
                left_b + sc_mul(bl_fp, row_bottom - yf),
                right_b + sc_mul(br_fp, row_bottom - yf),
            )
        } else {
            (left_b + bl_fp, right_b + br_fp)
        };

        let (left_min, left_max) = if yf < i_left {
            (bottom_left_a >> 16, top_left_a >> 16)
        } else if yf == i_left {
            (
                bottom_left_a.max(top_left_b) >> 16,
                top_left_a.max(bottom_left_b) >> 16,
            )
        } else {
            (top_left_b >> 16, bottom_left_b >> 16)
        };
        let (left_min, mut left_max) = (bound(left_min), bound(left_max));
        let (right_min, right_max) = if yf < i_right {
            (top_right_a >> 16, bottom_right_a >> 16)
        } else if yf == i_right {
            (
                top_right_a.min(bottom_right_b) >> 16,
                bottom_right_a.min(top_right_b) >> 16,
            )
        } else {
            (bottom_right_b >> 16, top_right_b >> 16)
        };
        let (mut right_min, right_max) = (bound(right_min), bound(right_max));
        if left_max > right_max {
            left_max = right_max;
        }
        if right_min < left_min {
            right_min = left_min;
        }

        let row_height = row_bottom - row_top;
        // The part of pixel `x` in this row that is right of the right edges.
        let right_excluded = |x: i64| {
            let mut e = 0;
            if yf <= i_right && row_bottom_right > row_top {
                e += (row_bottom_right - row_top)
                    - intersect_pixel_fp(
                        x,
                        row_top,
                        row_bottom_right,
                        top_right_a,
                        bottom_right_a,
                        tr_fp,
                        inv_tr,
                    );
            }
            if yf >= i_right && row_bottom > row_top_right {
                e += (row_bottom - row_top_right)
                    - intersect_pixel_fp(
                        x,
                        row_top_right,
                        row_bottom,
                        bottom_right_b,
                        top_right_b,
                        br_fp,
                        inv_br,
                    );
            }
            e
        };
        let mut x = left_min;
        while x <= left_max {
            let mut excluded = 0;
            if yf <= i_left && row_bottom_left > row_top {
                excluded += intersect_pixel_fp(
                    x,
                    row_top,
                    row_bottom_left,
                    bottom_left_a,
                    top_left_a,
                    tl_fp,
                    inv_tl,
                );
            }
            if yf >= i_left && row_bottom > row_top_left {
                excluded += intersect_pixel_fp(
                    x,
                    row_top_left,
                    row_bottom,
                    top_left_b,
                    bottom_left_b,
                    bl_fp,
                    inv_bl,
                );
            }
            if x >= right_min {
                excluded += right_excluded(x);
            }
            span(x, 1, y, (255 * (row_height - excluded)) >> 16);
            x += 1;
        }
        if x < right_min {
            span(x, right_min - x, y, (255 * row_height) >> 16);
            x = right_min;
        }
        while x <= right_max {
            span(x, 1, y, (255 * (row_height - right_excluded(x))) >> 16);
            x += 1;
        }

        left_a += tl_fp;
        left_b += bl_fp;
        right_a += tr_fp;
        right_b += br_fp;
        top_left_a = left_a;
        top_right_a = right_a;
        yf += SC;
        row_top = yf;
    }
}

fn normalized(r: [f64; 4]) -> [f64; 4] {
    let (mut x, mut y, mut rw, mut rh) = (r[0], r[1], r[2], r[3]);
    if rw < 0.0 {
        x += rw;
        rw = -rw;
    }
    if rh < 0.0 {
        y += rh;
        rh = -rh;
    }
    [x, y, rw, rh]
}

/// QRasterPaintEngineState::flags.tx_noshear (qt_scaleForTransform): a rectangle is the
/// rasterizer's line only under a transform that keeps its corners square.
pub(crate) fn no_shear(m: [f64; 6]) -> bool {
    let fuzzy = |a: f64, b: f64| (a - b).abs() * 1e12 <= a.abs().min(b.abs());
    if m[1] == 0.0 && m[2] == 0.0 {
        (m[0] == 1.0 && m[3] == 1.0) || fuzzy(m[0].abs(), m[3].abs())
    } else {
        fuzzy(m[0] * m[0] + m[2] * m[2], m[1] * m[1] + m[3] * m[3])
            && (m[0] * m[1] + m[2] * m[3]).abs() <= 1e-12
    }
}

/// QRasterizer::rasterizeLine(a, b, h / w) of a rectangle under the scale/translate transform
/// `m` on a `cw` x `ch` device: `span(x, y, len, coverage)` gets its spans. False when the
/// rectangle has to be filled as a path instead.
pub(crate) fn rect_spans(
    r: [f64; 4],
    m: [f64; 6],
    cw: usize,
    ch: usize,
    span: &mut dyn FnMut(i64, i64, i64, u32),
) -> bool {
    let [x, y, rw, rh] = normalized(r);
    if rw == 0.0 || rh == 0.0 {
        return true;
    }
    if m[1] != 0.0 || m[2] != 0.0 || !no_shear(m) {
        return false;
    }
    let mp = |px: f64, py: f64| [m[0] * px + m[4], m[3] * py + m[5]];
    let (a, b) = (mp(x, y + rh * 0.5), mp(x + rw, y + rh * 0.5));
    let (iw, ih) = (cw, ch);
    let (cw, ch) = (cw as f64, ch as f64);
    let mut width = rh / rw;
    let offs = [
        (b[1] - a[1]).abs() * width * 0.5,
        (b[0] - a[0]).abs() * width * 0.5,
    ];
    let inside = |p: [f64; 2]| {
        p[0] >= -offs[0] && p[0] <= cw + offs[0] && p[1] >= -offs[1] && p[1] <= ch + offs[1]
    };
    if a == b || !inside(a) || !inside(b) {
        return false;
    }
    let q26 = |p1: f64, p2: f64| (p1 * 64.0) as i64 == (p2 * 64.0) as i64;
    let (mut pa, mut pb) = (a, b);
    if q26(pa[1], pb[1]) {
        let xm = (pa[0] + pb[0]) * 0.5;
        let dx = (pb[0] - pa[0]).abs() * 0.5;
        let dy = width * dx;
        let y0 = pa[1];
        pa = [xm, y0 - dy];
        pb = [xm, y0 + dy];
        width = 1.0 / width;
    }
    if !q26(pa[0], pb[0]) {
        return false;
    }
    if pa[1] > pb[1] {
        std::mem::swap(&mut pa, &mut pb);
    }
    let half = 0.5 * width * (pb[1] - pa[1]);
    let left = (pa[0] - half).clamp(0.0, cw);
    let right = (pa[0] + half).clamp(0.0, cw);
    pa[1] = pa[1].clamp(0.0, ch);
    pb[1] = pb[1].clamp(0.0, ch);
    if q26(left, right) || q26(pa[1], pb[1]) {
        return true;
    }
    const F: i64 = 65536;
    let fx = |v: f64| (v * F as f64) as i64;
    let (il, ir) = (left as i64, right as i64);
    let lw = (il + 1) * F - fx(left);
    let rwid = fx(right) - ir * F;
    let mut spans: Vec<(i64, i64, i64)> = Vec::new();
    if il == ir {
        let c = if lw == F {
            rwid * 255
        } else {
            (rwid + lw - F) * 255
        };
        spans.push((il, 1, c));
    } else {
        if lw == F {
            spans.push((il, ir - il, lw * 255));
        } else {
            spans.push((il, 1, lw * 255));
            if ir - il > 1 {
                spans.push((il + 1, ir - il - 1, 255 * F));
            }
        }
        if rwid != 0 {
            spans.push((ir, 1, rwid * 255));
        }
    }
    let (ytop, ybot) = ((pa[1] as i64) * F, (pb[1] as i64) * F);
    let (ya, yb) = (fx(pa[1]), fx(pb[1]));
    let mut yf = ytop;
    while yf <= ybot {
        let row_h = (yf + F).min(yb) - yf.max(ya);
        let yy = yf >> 16;
        if yy > ih as i64 - 1 {
            break;
        }
        for &(sx, len, c) in &spans {
            let cov = ((row_h * c) >> 16) >> 16;
            if cov != 0 && len != 0 {
                span(sx, yy, len, cov as u32);
            }
        }
        yf += F;
    }
    let _ = iw;
    true
}

/// QPaintEngineEx::stroke for a non-cosmetic pen: the stroke outline of `path` in the path's own
/// space (QDashStroker feeding QStroker), to be filled with the pen brush under the painter
/// transform. `clip` is the device rect mapped into that space and `scale` is
/// qt_scaleForTransform of the painter transform. `true` comes with a pattern that repeats more
/// than QDashStroker::repetitionLimit times: the line is solid and the pen colour has half its
/// alpha. None when nothing is stroked.
#[allow(clippy::too_many_arguments)]
pub(crate) fn pen_outline(
    path: &Path,
    width: f64,
    cap: Cap,
    join: Join,
    miter_limit: f64,
    dash: &[f64],
    dash_offset: f64,
    clip: [f64; 4],
    scale: f64,
) -> Option<(Path, bool)> {
    if path.el.is_empty() || !(width.is_finite() && miter_limit.is_finite()) {
        return None;
    }
    let mut half = false;
    let mut dashed = None;
    if !dash.is_empty() {
        if !dash_offset.is_finite() || dash.iter().any(|v| !v.is_finite()) {
            return None;
        }
        // QPen::setDashPattern pads an odd pattern with 1.
        let mut pattern = dash.to_vec();
        if pattern.len() % 2 == 1 {
            pattern.push(1.0);
        }
        let pattern_length = pattern.iter().take(32).map(|v| v.max(0.0)).sum::<f64>() * width;
        if pattern_length.abs() <= 1e-12 {
            return None;
        }
        if dash_extent(path, width, clip) / pattern_length > DASH_REPETITION_LIMIT {
            half = true;
        } else {
            let thr = if scale == 0.0 { 0.5 } else { 0.5 / scale };
            dashed = Some(dash_path(
                path,
                width,
                miter_limit,
                &pattern,
                dash_offset,
                clip,
                thr,
            ));
        }
    }
    let path = dashed.as_ref().unwrap_or(path);
    if path.el.is_empty() {
        return None;
    }
    Some((stroke_outline(path, width, cap, join, miter_limit), half))
}

/// qPremultiplyWithExtraAlpha(color, intOpacity) -> premultiplied ARGB32 (8-bit path at full opacity).
pub fn solid_pm(color: [u8; 4], int_opacity: u32) -> [u32; 4] {
    if int_opacity == 0 {
        return [0; 4];
    }
    if int_opacity >= 256 {
        let a = color[3] as u32;
        let pm = [0, 1, 2].map(|k| byte_mul(color[k] as u32, a));
        return [pm[0], pm[1], pm[2], a];
    }
    let div65535 = |x: u64| (x + (x >> 16) + 0x8000) >> 16;
    let div257 = |x: u64| (x + 128 - ((x + 128) >> 8)) >> 8;
    let a16 = (color[3] as u64 * 257 * int_opacity as u64) >> 8;
    let pm = [0, 1, 2].map(|k| div257(div65535(color[k] as u64 * 257 * a16)) as u32);
    [pm[0], pm[1], pm[2], div257(a16) as u32]
}

/// Solid-color span blend (qt_bitmapblit/comp_func_solid_SourceOver with coverage).
pub(crate) fn blend_span(img: &mut Canvas, pm: [u32; 4], x: i64, y: i64, len: i64, cov: u32) {
    let w = img.w;
    if y < 0 || y >= img.h as i64 {
        return;
    }
    let a = pm[3];
    let c = if cov & a == 255 {
        None
    } else if cov != 255 {
        Some(pm.map(|v| byte_mul(v, cov)))
    } else {
        Some(pm)
    };
    for px in x.max(0)..(x + len).min(w as i64) {
        let d = &mut img.px[y as usize * w + px as usize];
        match c {
            None => *d = [pm[0] as u8, pm[1] as u8, pm[2] as u8, pm[3] as u8],
            Some(c) => {
                let ia = 255 - c[3];
                for k in 0..4 {
                    d[k] = (c[k] + byte_mul(d[k] as u32, ia)) as u8;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PI: f64 = std::f64::consts::PI;

    fn near(a: D, b: D, eps: f64) -> bool {
        (a[0] - b[0]).abs() <= eps && (a[1] - b[1]).abs() <= eps
    }

    fn px(c: &Context, x: usize, y: usize) -> [u8; 4] {
        c.img.px[y * c.img.w + x]
    }

    fn alpha_sum(c: &Context) -> f64 {
        c.img.px.iter().map(|p| p[3] as f64 / 255.0).sum()
    }

    fn rect_path(x: f64, y: f64, w: f64, h: f64) -> Path {
        let mut p = Path::default();
        p.move_to(x, y);
        p.line_to(x + w, y);
        p.line_to(x + w, y + h);
        p.line_to(x, y + h);
        p.close_path();
        p
    }

    fn cubic_ends(p: &Path) -> Vec<D> {
        p.el.iter()
            .filter_map(|e| {
                if let El::Cubic(_, _, e) = e {
                    Some(*e)
                } else {
                    None
                }
            })
            .collect()
    }

    fn filled(pts: &[(f64, f64)]) -> Vec<u8> {
        let mut p = Path::default();
        p.move_to(pts[0].0, pts[0].1);
        for q in &pts[1..] {
            p.line_to(q.0, q.1);
        }
        let mut ctx = Context::new(60, 40);
        ctx.fill(&p, [255, 0, 0, 255], false);
        ctx.img.px.iter().map(|p| p[3]).collect()
    }

    /// A path with points 1e9 px away fills the canvas like the same edges cut off nearby,
    /// and the rasterizer is handed no edge a billion scanlines long.
    #[test]
    fn far_outline_fills_like_its_near_part() {
        let e9 = 1_000_000_000 << 6;
        let outline = [vec![[672, -e9], [e9, -e9], [e9, e9], [672, e9]]];
        let cut = in_range(&outline, 60, 40);
        assert_eq!(
            cut[..],
            [vec![[672, 2624], [672, -64], [3904, -64], [3904, 2624]]],
            "clipped to one pixel around the 60 x 40 canvas"
        );
        let near = [vec![[672, -6400], [12800, -6400], [12800, 6400]]];
        assert_eq!(in_range(&near, 60, 40)[..], near, "a near outline is kept");
        // An upright edge at x = 10.5 and a slanted one through (0, 30) and (40, 0).
        let far = filled(&[(10.5, -1e9), (1e9, -1e9), (1e9, 1e9), (10.5, 1e9)]);
        let near = filled(&[
            (10.5, -100.0),
            (200.0, -100.0),
            (200.0, 100.0),
            (10.5, 100.0),
        ]);
        assert_eq!(far, near);
        assert_eq!(far[20 * 60 + 9..20 * 60 + 12], [0, 128, 255]);
        let k = 2.5e7;
        let far = filled(&[
            (-40.0 * k, 30.0 + 30.0 * k),
            (40.0 + 40.0 * k, -30.0 * k),
            (1e9, 1e9),
        ]);
        let near = filled(&[(-40.0, 60.0), (80.0, -30.0), (80.0, 60.0)]);
        let worst = far.iter().zip(&near).map(|(a, b)| a.abs_diff(*b)).max();
        assert!(worst <= Some(1), "coverage differs by {worst:?}");
        assert_eq!((far[0], far[39 * 60 + 59]), (0, 255));
        // The audit's path: a sliver a billion pixels away covers nothing here.
        let sliver = filled(&[(1e9, 0.0), (24.0, 1e9), (1e9, 14.0)]);
        assert!(sliver.iter().all(|a| *a == 0));
    }

    // QPainterPath::moveTo: a moveTo right after a moveTo replaces it.
    #[test]
    fn move_to_replaces_trailing_move() {
        let mut p = Path::default();
        p.move_to(1.0, 1.0);
        p.move_to(2.0, 3.0);
        assert_eq!(p.el.len(), 1);
        assert!(matches!(p.el[0], El::Move([2.0, 3.0])));
        assert_eq!(p.current_position(), Some([2.0, 3.0]));
    }

    // QQuickContext2D::lineTo: empty path -> moveTo(pt); same point as currentPosition() -> nothing.
    #[test]
    fn line_to_empty_and_repeated_point() {
        let mut p = Path::default();
        p.line_to(5.0, 5.0);
        assert!(matches!(p.el[..], [El::Move([5.0, 5.0])]));
        p.line_to(5.0, 5.0);
        assert_eq!(p.el.len(), 1);
        p.line_to(7.0, 5.0);
        assert!(matches!(p.el[..], [El::Move(_), El::Line([7.0, 5.0])]));
    }

    // QQuickContext2D::bezierCurveTo: empty path -> moveTo(cp1); end == currentPosition() -> nothing.
    #[test]
    fn bezier_curve_to_empty_and_degenerate() {
        let mut p = Path::default();
        p.bezier_curve_to(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
        assert!(matches!(
            p.el[..],
            [
                El::Move([1.0, 2.0]),
                El::Cubic([1.0, 2.0], [3.0, 4.0], [5.0, 6.0])
            ]
        ));
        p.bezier_curve_to(9.0, 9.0, 8.0, 8.0, 5.0, 6.0);
        assert_eq!(p.el.len(), 2);
    }

    // QPainterPathPrivate::close: LineTo(subpath start) unless the last point already is the start.
    #[test]
    fn close_path_returns_to_subpath_start() {
        let mut p = Path::default();
        p.move_to(0.0, 0.0);
        p.line_to(10.0, 0.0);
        p.move_to(20.0, 20.0);
        p.line_to(30.0, 20.0);
        p.line_to(30.0, 30.0);
        p.close_path();
        assert!(matches!(p.el.last(), Some(El::Line([20.0, 20.0]))));
        let n = p.el.len();
        p.close_path();
        assert_eq!(p.el.len(), n, "closing a closed subpath adds nothing");
    }

    // QQuickContext2D::closePath closes when boundingRect has a non-zero width OR height:
    // a vertical segment (width 0) still gets its return line.
    #[test]
    fn close_path_on_zero_width_bounds() {
        let mut p = Path::default();
        p.close_path();
        assert!(p.el.is_empty());
        p.move_to(4.0, 0.0);
        p.line_to(4.0, 10.0);
        p.close_path();
        assert!(matches!(
            p.el[..],
            [El::Move(_), El::Line([4.0, 10.0]), El::Line([4.0, 0.0])]
        ));
    }

    // QPainterPath::addRect: moveTo(x, y) + four LineTo back to the origin.
    #[test]
    fn add_rect_elements() {
        let mut p = Path::default();
        p.add_rect(1.0, 2.0, 3.0, 4.0);
        assert!(matches!(
            p.el[..],
            [
                El::Move([1.0, 2.0]),
                El::Line([4.0, 2.0]),
                El::Line([4.0, 6.0]),
                El::Line([1.0, 6.0]),
                El::Line([1.0, 2.0])
            ]
        ));
    }

    // QTransform::map: x' = m11 x + m21 y + dx, y' = m12 x + m22 y + dy, applied to every point.
    #[test]
    fn mapped_applies_affine_to_all_points() {
        let mut p = Path::default();
        p.move_to(1.0, 2.0);
        p.bezier_curve_to(3.0, 0.0, 0.0, 3.0, 2.0, 2.0);
        let m = p.mapped([0.0, 1.0, -1.0, 0.0, 5.0, 0.0]);
        assert!(matches!(m.el[0], El::Move([3.0, 1.0])));
        assert!(matches!(
            m.el[1],
            El::Cubic([5.0, 3.0], [2.0, 0.0], [3.0, 2.0])
        ));
    }

    // QQuickContext2D::arc on an empty path: arcMoveTo the start angle, then a full-circle
    // arcTo = 4 quadrant cubics (qt_curves_for_arc) closing at the start point.
    #[test]
    fn arc_full_circle() {
        let mut p = Path::default();
        p.arc(20.0, 10.0, 5.0, 0.0, 2.0 * PI, false);
        assert!(matches!(p.el[0], El::Move(s) if near(s, [25.0, 10.0], 1e-9)));
        let ends = cubic_ends(&p);
        assert_eq!(ends.len(), 4);
        assert_eq!(p.el.len(), 5);
        for e in &ends {
            assert!(((e[0] - 20.0).hypot(e[1] - 10.0) - 5.0).abs() < 1e-9);
        }
        assert!(near(*ends.last().unwrap(), [25.0, 10.0], 1e-9));
    }

    // Canvas angles grow clockwise (y down): 0 -> PI/2 clockwise is the quarter through
    // the lower right; anticlockwise is the other three quarters (QQuickContext2D::arc).
    #[test]
    fn arc_direction() {
        let mut cw = Path::default();
        cw.arc(0.0, 0.0, 10.0, 0.0, PI / 2.0, false);
        let ends = cubic_ends(&cw);
        assert_eq!(ends.len(), 1);
        assert!(near(ends[0], [0.0, 10.0], 1e-5));
        let mut acw = Path::default();
        acw.arc(0.0, 0.0, 10.0, 0.0, PI / 2.0, true);
        let ends = cubic_ends(&acw);
        assert_eq!(ends.len(), 3);
        assert!(near(ends[0], [0.0, -10.0], 1e-5));
        assert!(near(ends[1], [-10.0, 0.0], 1e-5));
        assert!(near(ends[2], [0.0, 10.0], 1e-5));
    }

    // QQuickContext2D::arc: non-finite args or sar == ear do nothing; radius 0 on a non-empty
    // path is lineTo(center).
    #[test]
    fn arc_degenerate_arguments() {
        let mut p = Path::default();
        p.arc(1.0, 1.0, 5.0, 1.0, 1.0, false);
        p.arc(f64::NAN, 1.0, 5.0, 0.0, 1.0, false);
        p.arc(1.0, 1.0, f64::INFINITY, 0.0, 1.0, false);
        assert!(p.el.is_empty());
        p.move_to(0.0, 0.0);
        p.arc(7.0, 3.0, 0.0, 0.0, 1.0, false);
        assert!(matches!(p.el[..], [El::Move(_), El::Line([7.0, 3.0])]));
    }

    // QQuickContext2D::arcTo / addArcTo: the arc is tangent to p0->p1 and p1->p2; it starts
    // with lineTo(first tangent point) and ends on the second tangent point.
    #[test]
    fn arc_to_points_tangent_arc() {
        let mut p = Path::default();
        p.move_to(0.0, 0.0);
        p.arc_to_points(10.0, 0.0, 10.0, 10.0, 5.0);
        assert!(matches!(p.el[1], El::Line(t) if near(t, [5.0, 0.0], 1e-9)));
        assert!(near(p.current_position().unwrap(), [10.0, 5.0], 1e-6));
        let ends = cubic_ends(&p);
        assert!(!ends.is_empty());
        for e in ends {
            assert!(((e[0] - 5.0).hypot(e[1] - 5.0) - 5.0).abs() < 1e-6);
        }
    }

    // QQuickContext2D::arcTo: empty path -> moveTo(p1); p1 == p0, p1 == p2 or radius 0 ->
    // lineTo(p1); collinear points -> lineTo(p1) (addArcTo, HTML5 4.8.11.1.8).
    #[test]
    fn arc_to_points_degenerate_cases() {
        let mut p = Path::default();
        p.arc_to_points(3.0, 4.0, 9.0, 9.0, 2.0);
        assert!(matches!(p.el[..], [El::Move([3.0, 4.0])]));

        let mut p = Path::default();
        p.move_to(0.0, 0.0);
        p.arc_to_points(5.0, 0.0, 5.0, 5.0, 0.0);
        assert!(matches!(p.el[..], [El::Move(_), El::Line([5.0, 0.0])]));

        let mut p = Path::default();
        p.move_to(0.0, 0.0);
        p.arc_to_points(5.0, 0.0, 5.0, 0.0, 3.0);
        assert!(matches!(p.el[..], [El::Move(_), El::Line([5.0, 0.0])]));

        let mut p = Path::default();
        p.move_to(0.0, 0.0);
        p.arc_to_points(5.0, 0.0, 10.0, 0.0, 3.0);
        assert!(matches!(p.el[..], [El::Move(_), El::Line([5.0, 0.0])]));
    }

    // Antialiased QPainter::fillPath of a pixel-aligned rect: full coverage inside, none outside.
    #[test]
    fn fill_pixel_aligned_rect_is_exact() {
        let mut c = Context::new(10, 10);
        c.fill(&rect_path(2.0, 3.0, 4.0, 5.0), [255, 0, 0, 255], false);
        for y in 0..10 {
            for x in 0..10 {
                let inside = (2..6).contains(&x) && (3..8).contains(&y);
                assert_eq!(
                    px(&c, x, y),
                    if inside { [255, 0, 0, 255] } else { [0; 4] },
                    "({x},{y})"
                );
            }
        }
    }

    // qgrayraster area coverage: an edge through the middle of a pixel covers it by half.
    #[test]
    fn fill_half_pixel_edge_coverage() {
        let mut c = Context::new(8, 4);
        c.fill(&rect_path(1.5, 0.0, 3.0, 4.0), [0, 0, 0, 255], false);
        assert!((127..=129).contains(&px(&c, 1, 2)[3]));
        assert_eq!(px(&c, 2, 2)[3], 255);
        assert_eq!(px(&c, 3, 2)[3], 255);
        assert!((127..=129).contains(&px(&c, 4, 2)[3]));
        assert_eq!(px(&c, 0, 2)[3], 0);
        assert_eq!(px(&c, 5, 2)[3], 0);
    }

    // Qt::WindingFill vs Qt::OddEvenFill: two same-direction overlapping squares fill the
    // overlap (winding 2) only under WindingFill.
    #[test]
    fn fill_rule_winding_vs_odd_even() {
        let mut p = rect_path(0.0, 0.0, 10.0, 10.0);
        let q = rect_path(5.0, 5.0, 10.0, 10.0);
        p.el.extend(q.el);
        for (even_odd, overlap) in [(false, 255), (true, 0)] {
            let mut c = Context::new(16, 16);
            c.fill(&p, [0, 0, 0, 255], even_odd);
            assert_eq!(px(&c, 7, 7)[3], overlap, "even_odd={even_odd}");
            assert_eq!(px(&c, 2, 2)[3], 255);
            assert_eq!(px(&c, 12, 12)[3], 255);
            assert_eq!(px(&c, 12, 2)[3], 0);
        }
    }

    // An inner contour of opposite direction is a hole under both fill rules.
    #[test]
    fn fill_reversed_inner_contour_is_hole() {
        let mut p = rect_path(0.0, 0.0, 12.0, 12.0);
        p.move_to(3.0, 3.0);
        p.line_to(3.0, 9.0);
        p.line_to(9.0, 9.0);
        p.line_to(9.0, 3.0);
        p.close_path();
        for even_odd in [false, true] {
            let mut c = Context::new(12, 12);
            c.fill(&p, [0, 0, 0, 255], even_odd);
            assert_eq!(px(&c, 6, 6)[3], 0);
            assert_eq!(px(&c, 1, 6)[3], 255);
            assert!((alpha_sum(&c) - 108.0).abs() < 1e-9);
        }
    }

    // QQuickContext2D::fill / QPainter::fillPath fill every subpath as if closed.
    #[test]
    fn fill_open_subpath_is_implicitly_closed() {
        let mut p = Path::default();
        p.move_to(0.0, 0.0);
        p.line_to(8.0, 0.0);
        p.line_to(8.0, 8.0);
        let mut c = Context::new(8, 8);
        c.fill(&p, [0, 0, 0, 255], false);
        // Triangle (0,0) (8,0) (8,8): area 32.
        assert!((alpha_sum(&c) - 32.0).abs() < 0.5);
        assert_eq!(px(&c, 6, 1)[3], 255);
        assert_eq!(px(&c, 1, 6)[3], 0);
    }

    // The raster engine clips to the device; a transparent brush paints nothing.
    #[test]
    fn fill_clips_to_device_and_skips_transparent() {
        let mut c = Context::new(6, 6);
        c.fill(&rect_path(-4.0, -4.0, 7.0, 20.0), [0, 0, 0, 255], false);
        assert!((alpha_sum(&c) - 18.0).abs() < 1e-9);
        let mut c = Context::new(6, 6);
        c.fill(&rect_path(0.0, 0.0, 6.0, 6.0), [255, 255, 255, 0], false);
        assert!(c.img.px.iter().all(|p| *p == [0; 4]));
    }

    // comp_func_solid_SourceOver: dst = src + BYTE_MUL(dst, 255 - src.alpha) on premultiplied
    // ARGB32 (qt_div_255 rounding).
    #[test]
    fn fill_source_over_blend() {
        let mut c = Context::new(2, 2);
        c.fill(&rect_path(0.0, 0.0, 2.0, 2.0), [0, 0, 255, 255], false);
        c.fill(&rect_path(0.0, 0.0, 2.0, 2.0), [255, 0, 0, 128], false);
        assert_eq!(px(&c, 1, 1), [128, 0, 127, 255]);
    }

    // qPremultiplyWithExtraAlpha: full opacity is the 8-bit premultiply; opacity 0 is
    // transparent; 128/256 of opaque white is multiplyAlpha256 + qt_div_257 = 127.
    #[test]
    fn solid_pm_premultiply_with_extra_alpha() {
        assert_eq!(solid_pm([255, 0, 0, 128], 256), [128, 0, 0, 128]);
        assert_eq!(solid_pm([10, 20, 30, 255], 256), [10, 20, 30, 255]);
        assert_eq!(solid_pm([255, 255, 255, 255], 0), [0; 4]);
        assert_eq!(solid_pm([255, 255, 255, 255], 128), [127, 127, 127, 127]);
    }

    // A filled circle covers pi r^2 minus the inscribed-polygon loss of QOutlineMapper's
    // QBezier::addToPolygon flattening (curve threshold 0.25 px): at most 2/3 * 0.25 * 2 pi r.
    #[test]
    fn fill_circle_area() {
        let mut p = Path::default();
        p.arc(16.0, 16.0, 10.0, 0.0, 2.0 * PI, false);
        let mut c = Context::new(32, 32);
        c.fill(&p, [0, 0, 0, 255], false);
        let a = alpha_sum(&c);
        let max_loss = 2.0 / 3.0 * 0.25 * 2.0 * PI * 10.0;
        assert!(a < PI * 100.0 + 0.5 && PI * 100.0 - a < max_loss, "{a}");
        assert_eq!(px(&c, 16, 16)[3], 255);
        assert_eq!(px(&c, 7, 7)[3], 0);
        assert_eq!(px(&c, 24, 16)[3], 255);
    }

    // QStroker caps: FlatCap ends at the endpoints, SquareCap extends by w/2, RoundCap adds
    // two half discs.
    #[test]
    fn stroke_caps() {
        let mut p = Path::default();
        p.move_to(10.0, 10.0);
        p.line_to(30.0, 10.0);
        for (cap, area) in [
            (Cap::Flat, 80.0),
            (Cap::Square, 96.0),
            (Cap::Round, 80.0 + 4.0 * PI),
        ] {
            let mut c = Context::new(40, 20);
            c.stroke(&p, [0, 0, 0, 255], 4.0, cap, Join::Miter, 10.0);
            assert!(
                (alpha_sum(&c) - area).abs() < 1.0,
                "{cap:?}: {}",
                alpha_sum(&c)
            );
        }
        let mut c = Context::new(40, 20);
        c.stroke(&p, [0, 0, 0, 255], 4.0, Cap::Flat, Join::Miter, 10.0);
        for y in 8..12 {
            for x in 10..30 {
                assert_eq!(px(&c, x, y)[3], 255);
            }
        }
        assert_eq!(px(&c, 9, 10)[3], 0);
        assert_eq!(px(&c, 30, 10)[3], 0);
        assert_eq!(px(&c, 20, 7)[3], 0);
        assert_eq!(px(&c, 20, 12)[3], 0);
    }

    // QStroker joins on a right angle: MiterJoin fills the outer corner square, BevelJoin
    // cuts it along the diagonal, RoundJoin rounds it (radius w/2 around the vertex).
    #[test]
    fn stroke_joins() {
        let mut p = Path::default();
        p.move_to(10.0, 10.0);
        p.line_to(30.0, 10.0);
        p.line_to(30.0, 30.0);
        let corner = |join| {
            let mut c = Context::new(40, 40);
            c.stroke(&p, [0, 0, 0, 255], 4.0, Cap::Flat, join, 10.0);
            px(&c, 31, 8)[3]
        };
        assert_eq!(corner(Join::Miter), 255);
        assert_eq!(corner(Join::Bevel), 0);
        let round = corner(Join::Round);
        assert!(round > 0 && round < 128, "{round}");
    }

    fn dashed(
        p: &Path,
        w: usize,
        h: usize,
        width: f64,
        join: Join,
        dash: &[f64],
        offset: f64,
    ) -> Context {
        let mut c = Context::new(w, h);
        c.stroke_dashed(
            p,
            [0, 0, 0, 255],
            width,
            Cap::Flat,
            join,
            10.0,
            dash,
            offset,
        );
        c
    }

    fn assert_runs(c: &Context, y: usize, on: &[(usize, usize)], off: &[(usize, usize)]) {
        for &(a, b) in on {
            for x in a..b {
                assert_eq!(px(c, x, y)[3], 255, "on at ({x}, {y})");
            }
        }
        for &(a, b) in off {
            for x in a..b {
                assert_eq!(px(c, x, y)[3], 0, "off at ({x}, {y})");
            }
        }
    }

    // QDashStroker scales the pattern by the pen width: [3, 2] at width 2 is 6 px on, 4 px
    // off, starting at the subpath start, and every dash is capped on its own.
    #[test]
    fn stroke_dashed_wide_pen_leaves_gaps() {
        let mut p = Path::default();
        p.move_to(2.0, 10.0);
        p.line_to(62.0, 10.0);
        let c = dashed(&p, 64, 20, 2.0, Join::Miter, &[3.0, 2.0], 0.0);
        let on: Vec<_> = (0..6).map(|k| (2 + 10 * k, 8 + 10 * k)).collect();
        let off: Vec<_> = (0..6).map(|k| (8 + 10 * k, 12 + 10 * k)).collect();
        for y in [9, 10] {
            assert_runs(&c, y, &on, &off);
        }
        assert_runs(&c, 8, &[], &[(0, 64)]);
        assert_runs(&c, 11, &[], &[(0, 64)]);
    }

    // lineDashOffset is in pen-width units too and shifts the pattern backwards along the
    // path; a negative offset wraps into the pattern.
    #[test]
    fn stroke_dashed_wide_pen_offset() {
        let mut p = Path::default();
        p.move_to(2.0, 10.0);
        p.line_to(62.0, 10.0);
        let c = dashed(&p, 64, 20, 2.0, Join::Miter, &[3.0, 2.0], 1.0);
        assert_runs(&c, 10, &[(2, 6), (10, 16), (20, 26)], &[(6, 10), (16, 20)]);
        let c = dashed(&p, 64, 20, 2.0, Join::Miter, &[3.0, 2.0], -1.0);
        assert_runs(&c, 10, &[(4, 10), (14, 20)], &[(0, 4), (10, 14), (20, 24)]);
    }

    // A dash running over a vertex stays one subpath, so QStroker joins it (miter corner);
    // the pattern runs on along the next segment.
    #[test]
    fn stroke_dashed_dash_continues_across_corner() {
        let mut p = Path::default();
        p.move_to(10.0, 10.0);
        p.line_to(30.0, 10.0);
        p.line_to(30.0, 30.0);
        let c = dashed(&p, 40, 40, 4.0, Join::Miter, &[3.0, 1.0], 0.0);
        assert_runs(&c, 10, &[(10, 22), (26, 32)], &[(22, 26)]);
        assert_eq!(px(&c, 31, 8)[3], 255);
        for y in 18..22 {
            assert_runs(&c, y, &[], &[(28, 32)]);
        }
        for y in [12, 17, 22, 29] {
            assert_runs(&c, y, &[(28, 32)], &[]);
        }
    }

    // QPaintEngineEx::stroke: an all-zero pattern strokes nothing; one repeating more than
    // QDashStroker::repetitionLimit times over the extent is a half-alpha solid line.
    #[test]
    fn stroke_dashed_degenerate_patterns() {
        let mut p = Path::default();
        p.move_to(2.0, 10.0);
        p.line_to(62.0, 10.0);
        let c = dashed(&p, 64, 20, 2.0, Join::Miter, &[0.0, 0.0], 0.0);
        assert_eq!(alpha_sum(&c), 0.0);
        let c = dashed(&p, 64, 20, 2.0, Join::Miter, &[1e-4, 1e-4], 0.0);
        for x in 2..62 {
            assert_eq!(px(&c, x, 10)[3], 127, "{x}");
        }
    }

    // QPainter::drawRect under a scale/translate transform maps the rect, then fills it.
    #[test]
    fn fill_rect_xf_scaled_and_rotated() {
        let mut c = Context::new(10, 10);
        c.fill_rect_xf(
            [1.0, 1.0, 2.0, 2.0],
            [0, 0, 0, 255],
            256,
            [2.0, 0.0, 0.0, 2.0, 0.0, 1.0],
        );
        for y in 0..10 {
            for x in 0..10 {
                let inside = (2..6).contains(&x) && (3..7).contains(&y);
                assert_eq!(px(&c, x, y)[3], if inside { 255 } else { 0 }, "({x},{y})");
            }
        }
        // 90 degree rotation + translate (10, 0): (1,1)-(4,3) -> x 7..9, y 1..4.
        let mut c = Context::new(10, 10);
        c.fill_rect_xf(
            [1.0, 1.0, 3.0, 2.0],
            [0, 0, 0, 255],
            256,
            [0.0, 1.0, -1.0, 0.0, 10.0, 0.0],
        );
        for y in 0..10 {
            for x in 0..10 {
                let inside = (7..9).contains(&x) && (1..4).contains(&y);
                assert_eq!(px(&c, x, y)[3], if inside { 255 } else { 0 }, "({x},{y})");
            }
        }
        // Negative width/height normalize (QRectF::normalized) and empty rects draw nothing.
        let mut c = Context::new(10, 10);
        c.fill_rect_xf(
            [5.0, 5.0, -2.0, -2.0],
            [0, 0, 0, 255],
            256,
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        );
        c.fill_rect_xf(
            [0.0, 0.0, 0.0, 5.0],
            [0, 0, 0, 255],
            256,
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        );
        assert!((alpha_sum(&c) - 4.0).abs() < 1e-9);
        assert_eq!(px(&c, 3, 3)[3], 255);
    }

    // Filling a polygon under a painter transform equals filling the mapped polygon.
    #[test]
    fn fill_with_transform_matches_mapped_path() {
        let mut p = Path::default();
        p.move_to(1.0, 1.0);
        p.line_to(6.0, 2.0);
        p.line_to(3.0, 7.0);
        let m = [1.5, 0.3, -0.2, 1.2, 2.25, 0.5];
        let mut a = Context::new(16, 16);
        a.fill_opacity(&p, [0, 0, 0, 255], false, 256, Some(m));
        let mut b = Context::new(16, 16);
        b.fill(&p.mapped(m), [0, 0, 0, 255], false);
        assert!(a.img.px == b.img.px);
    }
}
