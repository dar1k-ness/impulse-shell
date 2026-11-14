//! Fill of a Shape with the CurveRenderer, each step ported from Qt 6.12: QQuadPath
//! (subPathsClosed, addCurvatureData), QSGCurveProcessor (solveIntersections, solveOverlaps,
//! processFill), QSGCurveFillNode::appendTriangle and shapecurve.{vert,frag}.

use super::*;
use crate::triangulate::{LinePath, sort_by_lt, triangulate};
use std::collections::HashMap;

bits::bit_flags! {
const FILL_ON_RIGHT: u8 = 1;
const CONVEX: u8 = 2;
}

bits::bit_flags! {
/// QQuadPath::PathHint
pub const PATH_FILL_ON_RIGHT: u32 = 0x8;
pub const PATH_NON_INTERSECTING: u32 = 0x20;
pub const PATH_NON_OVERLAPPING_CONTROL_POINT_TRIANGLES: u32 = 0x40;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FillSide {
    Undetermined,
    Right,
    Left,
    Both,
}

/// QQuadPath::Element
#[derive(Clone, Copy, Default, Debug)]
struct QElem {
    sp: V,
    cp: V,
    ep: V,
    line: bool,
    sub_start: bool,
    sub_end: bool,
    flags: u8,
    first_child: i32,
    num_children: i32,
}

impl QElem {
    fn is_convex(&self) -> bool {
        self.flags & CONVEX != 0
    }
    fn is_fill_on_right(&self) -> bool {
        self.flags & FILL_ON_RIGHT != 0
    }
    fn mid_point(&self) -> V {
        if self.line {
            mul(add(self.sp, self.ep), 0.5)
        } else {
            add(
                add(mul(self.sp, 0.25), mul(self.cp, 0.5)),
                mul(self.ep, 0.25),
            )
        }
    }
    /// The control point of a curve; of a line, a point on the side the fill is on.
    fn reference_point(&self) -> V {
        if self.line {
            let normal = [self.sp[1] - self.ep[1], self.ep[0] - self.sp[0]];
            if self.is_fill_on_right() {
                add(self.sp, normal)
            } else {
                sub(self.sp, normal)
            }
        } else {
            self.cp
        }
    }
    fn point_at_fraction(&self, t: f32) -> V {
        if self.line {
            add(self.sp, mul(sub(self.ep, self.sp), t))
        } else {
            let r = 1.0 - t;
            add(
                add(mul(self.sp, r * r), mul(self.cp, 2.0 * t * r)),
                mul(self.ep, t * t),
            )
        }
    }
    fn tangent_at_fraction(&self, t: f32) -> V {
        if self.line {
            sub(self.ep, self.sp)
        } else {
            add(
                mul(sub(self.cp, self.sp), (1.0 - t) * 2.0),
                mul(sub(self.ep, self.cp), t * 2.0),
            )
        }
    }
    fn segment_from_to(&self, t0: f32, t1: f32) -> QElem {
        if t0 <= 0.0 && t1 >= 1.0 {
            return *self;
        }
        let mut part = QElem {
            sp: self.point_at_fraction(t0),
            ep: self.point_at_fraction(t1),
            ..Default::default()
        };
        if self.line {
            part.cp = mul(add(part.sp, part.ep), 0.5);
            part.line = true;
        } else {
            let rcp = add(mul(self.cp, 1.0 - t0), mul(self.ep, t0));
            let segment_t = (t1 - t0) / (1.0 - t0);
            part.cp = add(mul(part.sp, 1.0 - segment_t), mul(rcp, segment_t));
        }
        part
    }
    fn reversed(&self) -> QElem {
        QElem {
            sp: self.ep,
            cp: self.cp,
            ep: self.sp,
            line: self.line,
            ..Default::default()
        }
    }
    fn extent(&self) -> f32 {
        let mut min = [self.sp[0].min(self.ep[0]), self.sp[1].min(self.ep[1])];
        let mut max = [self.sp[0].max(self.ep[0]), self.sp[1].max(self.ep[1])];
        if !self.line {
            min = [min[0].min(self.cp[0]), min[1].min(self.cp[1])];
            max = [max[0].max(self.cp[0]), max[1].max(self.cp[1])];
        }
        len(sub(max, min))
    }
    /// The fractions at which the curve meets the horizontal line at `y`.
    fn intersections_at_y(&self, y: f32, swap_xy: bool) -> ([f32; 2], usize) {
        let get_y = |p: V| if swap_xy { -p[0] } else { p[1] };
        let y0 = get_y(self.sp) - y;
        let y1 = get_y(self.cp) - y;
        let y2 = get_y(self.ep) - y;
        let mut fractions = [0.0; 2];
        let mut num_roots = 0;
        let a = y0 - (2.0 * y1) + y2;
        if a != 0.0 {
            let b = (y1 * y1) - (y0 * y2);
            if b >= 0.0 {
                let sqr = b.sqrt();
                let root1 = -(-y0 + y1 + sqr) / a;
                if root1.is_finite() && (0.0..=1.0).contains(&root1) {
                    fractions[num_roots] = root1;
                    num_roots += 1;
                }
                let root2 = (y0 - y1 + sqr) / a;
                if root2.is_finite() && root2 != root1 && (0.0..=1.0).contains(&root2) {
                    fractions[num_roots] = root2;
                    num_roots += 1;
                }
            }
        } else if y1 != y2 {
            let root1 = (y2 - (2.0 * y1)) / (2.0 * (y2 - y1));
            if root1.is_finite() && (0.0..=1.0).contains(&root1) {
                fractions[num_roots] = root1;
                num_roots += 1;
            }
        }
        (fractions, num_roots)
    }
}

fn cross_product(sp: V, p: V, ep: V) -> f32 {
    let v1 = sub(ep, sp);
    let v2 = sub(p, sp);
    (v2[0] * v1[1]) - (v2[1] * v1[0])
}

fn is_point_on_left(p: V, sp: V, ep: V) -> bool {
    cross_product(sp, p, ep) >= 0.0
}

fn is_point_on_line(p: V, sp: V, ep: V) -> bool {
    fnull_f(cross_product(sp, p, ep))
}

/// QQuadPath
#[derive(Clone, Default)]
struct QuadPath {
    el: Vec<QElem>,
    children: Vec<QElem>,
    winding: bool,
    hints: u32,
    cur: V,
    to_start: bool,
}

impl QuadPath {
    fn at(&self, i: i32) -> &QElem {
        if i < 0 {
            &self.children[(-(i + 1)) as usize]
        } else {
            &self.el[i as usize]
        }
    }
    fn index_of_child_at(&self, i: i32, child: i32) -> i32 {
        -(self.at(i).first_child + 1 + child)
    }
    fn move_to(&mut self, to: V) {
        self.to_start = true;
        self.cur = to;
    }
    fn line_to(&mut self, to: V) {
        self.add(Z, to, true);
    }
    fn quad_to(&mut self, control: V, to: V) {
        self.add(control, to, false);
    }
    fn add(&mut self, control: V, to: V, mut line: bool) {
        if fcmp_v(self.cur, to) {
            return;
        }
        line = line || near_line(control, self.cur, to);
        if !self.to_start
            && let Some(l) = self.el.last_mut()
        {
            l.sub_end = false;
        }
        self.el.push(QElem {
            sp: self.cur,
            cp: if line {
                mul(add(self.cur, to), 0.5)
            } else {
                control
            },
            ep: to,
            line,
            sub_start: self.to_start,
            sub_end: true,
            ..Default::default()
        });
        self.to_start = false;
        self.cur = to;
    }
    fn add_element(&mut self, e: QElem) {
        self.to_start = false;
        self.cur = e.ep;
        self.el.push(e);
    }

    /// QQuadPath::fromPainterPath
    fn from_painter_path(pp: &PPath, winding: bool, hints: u32) -> QuadPath {
        let mut q = QuadPath {
            winding,
            hints,
            ..Default::default()
        };
        let mut sp = [0.0; 2];
        for e in &pp.0 {
            match *e {
                PE::Move(p) => {
                    q.move_to(d2v(p));
                    sp = p;
                }
                PE::Line(p) => {
                    q.line_to(d2v(p));
                    sp = p;
                }
                PE::Cubic(c1, c2, ep) => {
                    let quads = to_quadratics(&Bez([sp, c1, c2, ep]));
                    let mut i = 1;
                    while i + 1 < quads.len() {
                        q.quad_to(d2v(quads[i]), d2v(quads[i + 1]));
                        i += 2;
                    }
                    sp = ep;
                }
            }
        }
        q
    }

    /// QQuadPath::subPathsClosed
    fn sub_paths_closed(&self) -> QuadPath {
        let mut res = self.clone();
        res.to_start = false;
        res.el = Vec::with_capacity(self.el.len());
        let mut sub_start: Option<usize> = None;
        let mut prev_element: Option<usize> = None;
        for (i, element) in self.el.iter().enumerate() {
            if element.sub_start {
                match sub_start {
                    Some(s) if self.el[i - 1].ep != self.el[s].sp => {
                        res.cur = self.el[i - 1].ep;
                        res.line_to(self.el[s].sp);
                        let end = res.el.last_mut().unwrap();
                        end.sub_end = true;
                        end.ep = self.el[s].sp;
                    }
                    _ => {
                        if let Some(p) = prev_element {
                            res.el[p].sub_end = true;
                        }
                    }
                }
                sub_start = Some(i);
            }
            res.el.push(*element);
            prev_element = Some(res.el.len() - 1);
        }
        if let Some(s) = sub_start {
            if self.el.last().unwrap().ep != self.el[s].sp {
                res.cur = self.el.last().unwrap().ep;
                res.line_to(self.el[s].sp);
            }
            let end = res.el.last_mut().unwrap();
            end.sub_end = true;
            end.ep = self.el[s].sp;
        }
        res
    }

    fn inside(&self, winding_number: i32) -> bool {
        if self.winding {
            winding_number != 0
        } else {
            winding_number % 2 != 0
        }
    }

    /// QQuadPath::contains; the subpaths are closed.
    fn contains(&self, point: V) -> bool {
        self.contains_range(point, 0, self.el.len() as i32 - 1)
    }

    fn contains_range(&self, point: V, from: i32, to: i32) -> bool {
        let mut winding_number = 0;
        for ei in from..=to {
            let e = &self.el[ei as usize];
            let mut dir = 1;
            let mut y1 = e.sp[1];
            let mut y2 = e.ep[1];
            if y2 < y1 {
                std::mem::swap(&mut y1, &mut y2);
                dir = -1;
            }
            if e.line {
                if point[1] < y1 || point[1] >= y2 || y1 == y2 {
                    continue;
                }
                let t = (point[1] - e.sp[1]) / (e.ep[1] - e.sp[1]);
                let x = e.sp[0] + t * (e.ep[0] - e.sp[0]);
                if x <= point[0] {
                    winding_number += dir;
                }
            } else {
                y1 = y1.min(e.cp[1]);
                y2 = y2.max(e.cp[1]);
                if point[1] < y1 || point[1] >= y2 {
                    continue;
                }
                let (ts, num_roots) = e.intersections_at_y(point[1], false);
                // Exactly one intersection to the left counts.
                let mut one_hit = false;
                let mut t_for_hit = -1.0;
                for &t in &ts[..num_roots] {
                    if e.point_at_fraction(t)[0] <= point[0] {
                        one_hit = !one_hit;
                        t_for_hit = t;
                    }
                }
                if one_hit {
                    let mut ty = e.tangent_at_fraction(t_for_hit)[1];
                    if fnull_f(ty) {
                        // A horizontal tangent is ambiguous: the second derivative decides.
                        let b = e.ep[1] - 2.0 * e.cp[1] + e.sp[1];
                        ty = if t_for_hit < 0.5 { b } else { -b };
                    }
                    if ty != 0.0 {
                        winding_number += if ty < 0.0 { -1 } else { 1 };
                    }
                }
            }
        }
        self.inside(winding_number)
    }

    /// QQuadPath::fillSideOf: `contains` for a point just left and just right of element
    /// `element_idx` at `element_t`.
    fn fill_side_of(&self, element_idx: i32, element_t: f32) -> FillSide {
        const TOLERANCE_T: f32 = 1e-3;
        let point = self.el[element_idx as usize].point_at_fraction(element_t);
        let tangent = self.el[element_idx as usize].tangent_at_fraction(element_t);

        let swap_xy = tangent[0].abs() > tangent[1].abs();
        let get_x = |p: V| if swap_xy { p[1] } else { p[0] };
        let get_y = |p: V| if swap_xy { -p[0] } else { p[1] };

        let mut winding_number = 0;
        for (i, e) in self.el.iter().enumerate() {
            let i = i as i32;
            let mut dir = 1;
            let mut y1 = get_y(e.sp);
            let mut y2 = get_y(e.ep);
            if y2 < y1 {
                std::mem::swap(&mut y1, &mut y2);
                dir = -1;
            }
            if e.line {
                if get_y(point) < y1 || get_y(point) >= y2 || y1 == y2 {
                    continue;
                }
                let t = (get_y(point) - get_y(e.sp)) / (get_y(e.ep) - get_y(e.sp));
                let x = get_x(e.sp) + t * (get_x(e.ep) - get_x(e.sp));
                if x <= get_x(point) && (i != element_idx || (t - element_t).abs() > TOLERANCE_T) {
                    winding_number += dir;
                }
            } else {
                y1 = y1.min(get_y(e.cp));
                y2 = y2.max(get_y(e.cp));
                if get_y(point) < y1 || get_y(point) >= y2 {
                    continue;
                }
                let (ts, num_roots) = e.intersections_at_y(get_y(point), swap_xy);
                let mut one_hit = false;
                let mut t_for_hit = -1.0;
                for &t in &ts[..num_roots] {
                    let x = get_x(e.point_at_fraction(t));
                    if x <= get_x(point)
                        && (i != element_idx || (t - element_t).abs() > TOLERANCE_T)
                    {
                        one_hit = !one_hit;
                        t_for_hit = t;
                    }
                }
                if one_hit {
                    winding_number += if get_y(e.tangent_at_fraction(t_for_hit)) < 0.0 {
                        -1
                    } else {
                        1
                    };
                }
            }
        }

        let mut left_winding_number = winding_number;
        let mut right_winding_number = winding_number;
        let dir = if get_y(tangent) < 0.0 { -1 } else { 1 };
        if dir > 0 {
            left_winding_number += dir;
        } else {
            right_winding_number += dir;
        }
        match (
            self.inside(left_winding_number),
            self.inside(right_winding_number),
        ) {
            (true, true) => FillSide::Both,
            (true, false) => FillSide::Left,
            (false, true) => FillSide::Right,
            (false, false) => FillSide::Undetermined,
        }
    }

    /// QQuadPath::coordinateOrderOfElement
    fn coordinate_order_of_element(&self, element: &QElem) -> FillSide {
        const CONVEX_CHECK_ERROR_MARGIN: f32 = 1.0 / 32.0;
        let base_line = sub(element.ep, element.sp);
        let mid_point = element.mid_point();
        // At its midpoint a quad's tangent is parallel to the baseline.
        let normal = norm([-base_line[1], base_line[0]]);
        let delta = (element.extent() / 100.0).min(CONVEX_CHECK_ERROR_MARGIN);
        let offset = mul(normal, delta);
        let to_right = self.contains(add(mid_point, offset));
        let to_left = self.contains(sub(mid_point, offset));
        match (to_right, to_left) {
            (true, true) => FillSide::Both,
            (true, false) => FillSide::Right,
            (false, true) => FillSide::Left,
            (false, false) => FillSide::Undetermined,
        }
    }

    /// hasOverlappingLines of qquadpath.cpp, with its indexing: the inner loop reads element
    /// `j`, not the `j`th line.
    fn has_overlapping_lines(&self) -> bool {
        let mut lines = 0;
        for e in &self.el {
            if e.line {
                let tangent = e.tangent_at_fraction(0.0);
                let is_horizontal = tangent[0].abs() > tangent[1].abs();
                let k = if is_horizontal { 0 } else { 1 };
                let (mut e1, mut e2) = (e.sp[k], e.ep[k]);
                if e1 > e2 {
                    std::mem::swap(&mut e1, &mut e2);
                }
                for line in &self.el[..lines] {
                    if is_point_on_line(e.cp, line.sp, line.ep) {
                        let (mut l1, mut l2) = (line.sp[k], line.ep[k]);
                        if l1 > l2 {
                            std::mem::swap(&mut l1, &mut l2);
                        }
                        if l1.max(e1) <= l2.min(e2) {
                            return true;
                        }
                    }
                }
                lines += 1;
            }
        }
        false
    }

    /// QQuadPath::addCurvatureData: which side of each element the fill is on, and whether a
    /// curve is convex.
    fn add_curvature_data(&mut self) {
        let is_single_sided = |s: FillSide| s == FillSide::Left || s == FillSide::Right;
        let flag_from_fill_side = |s: FillSide| {
            if s == FillSide::Right || s == FillSide::Both {
                FILL_ON_RIGHT
            } else {
                0
            }
        };
        const OVERLAP_TEST_LIMIT: usize = 64;
        let line_count = self.el.iter().filter(|e| e.line).count();
        let check_anomaly = line_count <= OVERLAP_TEST_LIMIT && self.has_overlapping_lines();
        let path_has_fill_on_right = self.hints & PATH_FILL_ON_RIGHT != 0;

        let mut flags = 0;
        for i in 0..self.el.len() {
            let element = self.el[i];
            if element.sub_start {
                if path_has_fill_on_right && !check_anomaly {
                    flags = FILL_ON_RIGHT;
                } else {
                    let mut fill_side = FillSide::Undetermined;
                    let mut j = i;
                    while !is_single_sided(fill_side) && j < self.el.len() {
                        let sub_elem = self.el[j];
                        if j > i && sub_elem.sub_start {
                            break;
                        }
                        fill_side = self.coordinate_order_of_element(&sub_elem);
                        j += 1;
                    }
                    flags = flag_from_fill_side(fill_side);
                }
            } else if check_anomaly {
                let fill_side = self.coordinate_order_of_element(&element);
                if is_single_sided(fill_side) {
                    flags = flag_from_fill_side(fill_side);
                }
            }

            if element.line {
                self.el[i].flags = flags;
            } else {
                let control_point_on_left = is_point_on_left(element.cp, element.sp, element.ep);
                let is_fill_on_right = flags & FILL_ON_RIGHT != 0;
                let is_convex = control_point_on_left == is_fill_on_right;
                self.el[i].flags = if is_convex { flags | CONVEX } else { flags };
            }
        }
    }

    /// QQuadPath::splitElementAt
    fn split_element_at(&mut self, index: i32) {
        let new_child_index = self.children.len() as i32;
        let parent = *self.at(index);
        let mp = parent.mid_point();
        let quad1 = QElem {
            sp: parent.sp,
            cp: mul(add(parent.sp, parent.cp), 0.5),
            ep: mp,
            sub_start: parent.sub_start,
            sub_end: false,
            flags: parent.flags,
            line: parent.line,
            ..Default::default()
        };
        let quad2 = QElem {
            sp: mp,
            cp: mul(add(parent.ep, parent.cp), 0.5),
            ep: parent.ep,
            sub_start: false,
            sub_end: parent.sub_end,
            flags: parent.flags,
            line: parent.line,
            ..Default::default()
        };
        self.children.push(quad1);
        self.children.push(quad2);
        let p = if index < 0 {
            &mut self.children[(-(index + 1)) as usize]
        } else {
            &mut self.el[index as usize]
        };
        p.num_children = 2;
        p.first_child = new_child_index;
    }

    fn collect_children(&self, e: &QElem, out: &mut Vec<i32>) {
        for i in e.first_child..e.first_child + e.num_children {
            let c = &self.children[i as usize];
            if c.num_children > 0 {
                self.collect_children(c, out);
            } else {
                out.push(-(i + 1));
            }
        }
    }

    /// The indices QQuadPath::iterateElements visits: the elements that are not split, in order.
    fn leaves(&self) -> Vec<i32> {
        let mut out = Vec::new();
        for (i, e) in self.el.iter().enumerate() {
            if e.num_children > 0 {
                self.collect_children(e, &mut out);
            } else {
                out.push(i as i32);
            }
        }
        out
    }
}

// ---------------------------------------------------------------- QSGCurveProcessor: overlaps

/// Whether lines `l1` and `l2` cross; the fractions (of `l1`, of `l2`) go to `solution`.
fn line_intersection(l1: [V; 2], l2: [V; 2], solution: Option<&mut Vec<(f32, f32)>>) -> bool {
    const EPS2: f64 = 1e-5;
    let a = l1[0][0];
    let b = l1[1][0] - l1[0][0];
    let c = l2[0][0];
    let d = l2[1][0] - l2[0][0];
    let e = l1[0][1];
    let f = l1[1][1] - l1[0][1];
    let g = l2[0][1];
    let h = l2[1][1] - l2[0][1];

    let det = d * f - b * h;
    if det == 0.0 {
        return false;
    }
    let s = (f * (a - c) - b * (e - g)) / det;
    let t = (h * (a - c) - d * (e - g)) / det;

    // An intersection at 0 counts, one at 1 does not.
    let intersecting = s >= 0.0 && s as f64 <= 1.0 - EPS2 && t >= 0.0 && t as f64 <= 1.0 - EPS2;
    if intersecting && let Some(sol) = solution {
        sol.push((t, s));
    }
    intersecting
}

fn check_line_triangle_overlap(mut triangle: [V; 3], line: [V; 2]) -> bool {
    let eps = 1.0 / 32.0;
    let s1 = det3(line[0], line[1], triangle[0]) < 0.0;
    let s2 = det3(line[0], line[1], triangle[1]) < 0.0;
    let s3 = det3(line[0], line[1], triangle[2]) < 0.0;
    if s1 == s2 && s2 == s3 {
        return false;
    }
    fix_winding(&mut triangle);
    for i in 0..3 {
        let ni = (i + 1) % 3;
        if check_edge(triangle[i], triangle[ni], line[0], eps)
            && check_edge(triangle[i], triangle[ni], line[1], eps)
        {
            return false;
        }
    }
    true
}

fn is_overlap(path: &QuadPath, e1: i32, e2: i32) -> bool {
    let (a, b) = (path.at(e1), path.at(e2));
    match (a.line, b.line) {
        (true, true) => line_intersection([a.sp, a.ep], [b.sp, b.ep], None),
        (true, false) => check_line_triangle_overlap([b.sp, b.cp, b.ep], [a.sp, a.ep]),
        (false, true) => check_line_triangle_overlap([a.sp, a.cp, a.ep], [b.sp, b.ep]),
        (false, false) => triangles_overlap([a.sp, a.cp, a.ep], [b.sp, b.cp, b.ep]),
    }
}

/// True if a change was made.
fn handle_overlap(path: &mut QuadPath, e1: i32, e2: i32, recursion_level: i32) -> bool {
    // As in Qt, only the first element is asked whether it is a line.
    if path.at(e1).line {
        return false;
    }
    if !is_overlap(path, e1, e2) {
        return false;
    }
    if recursion_level > 8 {
        return false;
    }

    if path.at(e1).num_children > 1 {
        let e11 = path.index_of_child_at(e1, 0);
        let e12 = path.index_of_child_at(e1, 1);
        handle_overlap(path, e11, e2, recursion_level + 1);
        handle_overlap(path, e12, e2, recursion_level + 1);
    } else if path.at(e2).num_children > 1 {
        let e21 = path.index_of_child_at(e2, 0);
        let e22 = path.index_of_child_at(e2, 1);
        handle_overlap(path, e1, e21, recursion_level + 1);
        handle_overlap(path, e1, e22, recursion_level + 1);
    } else {
        path.split_element_at(e1);
        let e11 = path.index_of_child_at(e1, 0);
        let e12 = path.index_of_child_at(e1, 1);
        let overlap1 = is_overlap(path, e11, e2);
        let overlap2 = is_overlap(path, e12, e2);
        if !overlap1 && !overlap2 {
            return true;
        }

        if path.at(e2).line {
            // Splitting a line does not help: e1 is split further.
            if overlap1 {
                handle_overlap(path, e11, e2, recursion_level + 1);
            }
            if overlap2 {
                handle_overlap(path, e12, e2, recursion_level + 1);
            }
        } else {
            path.split_element_at(e2);
            let e21 = path.index_of_child_at(e2, 0);
            let e22 = path.index_of_child_at(e2, 1);
            if overlap1 {
                handle_overlap(path, e11, e21, recursion_level + 1);
                handle_overlap(path, e11, e22, recursion_level + 1);
            }
            if overlap2 {
                handle_overlap(path, e12, e21, recursion_level + 1);
                handle_overlap(path, e12, e22, recursion_level + 1);
            }
        }
    }
    true
}

/// QSGCurveProcessor::solveOverlaps: curves whose control point triangles overlap are split.
fn solve_overlaps(path: &mut QuadPath) {
    if path.hints & PATH_NON_OVERLAPPING_CONTROL_POINT_TRIANGLES != 0 {
        return;
    }
    for (a, b) in find_overlapping_candidates(path) {
        handle_overlap(path, a, b, 0);
    }
    path.hints |= PATH_NON_OVERLAPPING_CONTROL_POINT_TRIANGLES;
}

/// QSGCurveProcessor::findOverlappingCandidates: pairs of elements whose bounding boxes meet.
fn find_overlapping_candidates(path: &QuadPath) -> Vec<(i32, i32)> {
    // xmin, xmax, ymin, ymax
    let rects: Vec<[f32; 4]> = path
        .el
        .iter()
        .map(|e| {
            [
                e.sp[0].min(e.cp[0]).min(e.ep[0]),
                e.sp[0].max(e.cp[0]).max(e.ep[0]),
                e.sp[1].min(e.cp[1]).min(e.ep[1]),
                e.sp[1].max(e.cp[1]).max(e.ep[1]),
            ]
        })
        .collect();
    let mut element_starts: Vec<usize> = (0..path.el.len()).collect();
    sort_by_lt(&mut element_starts, |&i, &j| rects[i][0] < rects[j][0]);
    let mut element_ends = element_starts.clone();
    sort_by_lt(&mut element_ends, |&i, &j| rects[i][1] < rects[j][1]);

    let mut pool: Vec<usize> = Vec::new();
    let mut overlapping = Vec::new();
    let mut first_element_end = 0;
    for &add_index in &element_starts {
        let new_r = rects[add_index];
        // Elements whose xmax is left of the new one leave the pool.
        while !pool.is_empty() && first_element_end < element_ends.len() {
            let remove_index = element_ends[first_element_end];
            if let Some(pos) = pool.iter().position(|&p| p == remove_index)
                && new_r[0] > rects[remove_index][1]
            {
                pool.remove(pos);
                first_element_end += 1;
            } else {
                break;
            }
        }

        for &i in &pool {
            let r1 = rects[i];
            let mut is_neighbor = false;
            if i == add_index + 1 {
                if !path.el[add_index].sub_end {
                    is_neighbor = true;
                }
            } else if add_index == i + 1 && !path.el[i].sub_end {
                is_neighbor = true;
            }
            // Neighbours that only share a point do not count; the others may just touch.
            if is_neighbor && (r1[3] <= new_r[2] || new_r[3] <= r1[2]) {
                continue;
            }
            if !is_neighbor && (r1[3] < new_r[2] || new_r[3] < r1[2]) {
                continue;
            }
            overlapping.push((i as i32, add_index as i32));
        }
        pool.push(add_index);
    }
    overlapping
}

// ---------------------------------------------------------------- QSGCurveProcessor: intersections

fn angle_between(v1: V, v2: V) -> f32 {
    let dot = v1[0] * v2[0] + v1[1] * v2[1];
    let cross = v1[0] * v2[1] - v1[1] * v2[0];
    cross.atan2(dot)
}

/// Where two quadratic curves cross, by Newton's method from nine starting points.
fn curves_intersecting(t1: [V; 3], t2: [V; 3], solutions: &mut Vec<(f32, f32)>) -> bool {
    const EPS: f64 = 1e-5;
    const EPS2: f64 = 1e-5;
    const MAX_ITERATIONS: i32 = 7;
    let d = |p: V| [p[0] as f64, p[1] as f64];
    let td1 = [d(t1[0]), d(t1[1]), d(t1[2])];
    let td2 = [d(t2[0]), d(t2[1]), d(t2[2])];

    // F = P1(t.x) - P2(t.y), zero at an intersection.
    let f = |t: D| -> D {
        std::array::from_fn(|k| {
            td1[0][k] * (1.0 - t[0]) * (1.0 - t[0])
                + 2.0 * td1[1][k] * (1.0 - t[0]) * t[0]
                + td1[2][k] * t[0] * t[0]
                - td2[0][k] * (1.0 - t[1]) * (1.0 - t[1])
                - 2.0 * td2[1][k] * (1.0 - t[1]) * t[1]
                - td2[2][k] * t[1] * t[1]
        })
    };
    // The Jacobi matrix dF/dt: x1 y1 / x2 y2.
    let j = |t: D| -> [f64; 4] {
        let row = |k: usize| {
            (
                td1[0][k] * (-2.0 * (1.0 - t[0]))
                    + 2.0 * td1[1][k] * (1.0 - 2.0 * t[0])
                    + td1[2][k] * 2.0 * t[0],
                -td2[0][k] * (-2.0 * (1.0 - t[1]))
                    - 2.0 * td2[1][k] * (1.0 - 2.0 * t[1])
                    - td2[2][k] * 2.0 * t[1],
            )
        };
        let (x1, y1) = row(0);
        let (x2, y2) = row(1);
        [x1, y1, x2, y2]
    };
    let solve = |a: [f64; 4], b: D| -> D {
        let [x1, y1, x2, y2] = a;
        let det = x1 * y2 - y1 * x2;
        let inv = [y2 / det, -y1 / det, -x2 / det, x1 / det];
        [inv[0] * b[0] + inv[1] * b[1], inv[2] * b[0] + inv[3] * b[1]]
    };

    const TREF: [D; 9] = [
        [0.0, 0.0],
        [0.5, 0.0],
        [1.0, 0.0],
        [0.0, 0.5],
        [0.5, 0.5],
        [1.0, 0.5],
        [0.0, 1.0],
        [0.5, 1.0],
        [1.0, 1.0],
    ];
    for mut t in TREF {
        let mut err = 1.0;
        let mut fval = f(t);
        let mut i = 0;
        while err > EPS && i < MAX_ITERATIONS {
            let s = solve(j(t), fval);
            t = [t[0] - s[0], t[1] - s[1]];
            fval = f(t);
            err = fval[0].abs() + fval[1].abs();
            i += 1;
        }
        // An intersection at 0 counts, one at 1 does not.
        if err < EPS
            && t[0] >= 0.0
            && t[0] <= 1.0 - 10.0 * EPS2
            && t[1] >= 0.0
            && t[1] <= 1.0 - 10.0 * EPS2
        {
            let known = solutions.iter().any(|s| {
                (s.0 as f64 - t[0]).abs() < 10.0 * EPS2 && (s.1 as f64 - t[1]).abs() < 10.0 * EPS2
            });
            if !known {
                solutions.push((t[0] as f32, t[1] as f32));
            }
        }
    }
    !solutions.is_empty()
}

fn is_intersecting(path: &QuadPath, e1: i32, e2: i32, solutions: &mut Vec<(f32, f32)>) -> bool {
    let (a, b) = (path.at(e1), path.at(e2));
    if a.line && b.line {
        line_intersection([a.sp, a.ep], [b.sp, b.ep], Some(solutions))
    } else {
        curves_intersecting([a.sp, a.cp, a.ep], [b.sp, b.cp, b.ep], solutions)
    }
}

/// The first and the last element of every subpath.
fn sub_path_bounds(path: &QuadPath) -> (Vec<i32>, Vec<i32>) {
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    for (i, e) in path.el.iter().enumerate() {
        if e.sub_start {
            starts.push(i as i32);
        }
        if e.sub_end {
            ends.push(i as i32);
        }
    }
    (starts, ends)
}

/// QSGCurveProcessor::removeNestedSubpaths: a subpath inside another one that fills (or cuts)
/// the same way goes. True if the path changed.
fn remove_nested_subpaths(path: &mut QuadPath) -> bool {
    if !path.winding {
        return false;
    }
    let (starts, ends) = sub_path_bounds(path);
    let count = starts.len();
    if count < 2 {
        return false;
    }

    let mut is_inside = Vec::with_capacity(count * count);
    let mut is_any_inside = false;
    for i in 0..count {
        for j in 0..count {
            if i == j {
                is_inside.push(false);
            } else {
                let inside =
                    path.contains_range(path.el[starts[i] as usize].sp, starts[j], ends[j]);
                is_inside.push(inside);
                is_any_inside |= inside;
            }
        }
    }
    if !is_any_inside {
        return false;
    }

    let clockwise: Vec<bool> = (0..count)
        .map(|i| {
            let mut sum_product = 0.0f32;
            for j in starts[i]..=ends[i] {
                let (sp, ep) = (path.el[j as usize].sp, path.el[j as usize].ep);
                sum_product += (ep[0] - sp[0]) * (ep[1] + sp[1]);
            }
            sum_product > 0.0
        })
        .collect();

    let is_filled: Vec<bool> = (0..count)
        .map(|i| {
            let mut crossings = if clockwise[i] { 1 } else { -1 };
            for j in 0..count {
                if is_inside[i * count + j] {
                    crossings += if clockwise[j] { 1 } else { -1 };
                }
            }
            crossings != 0
        })
        .collect();

    // The innermost subpath around a subpath; None for a toplevel one.
    let find_closest_outer_subpath = |sub_path: usize| -> Option<usize> {
        let candidates: Vec<usize> = (0..count)
            .filter(|&i| is_inside[sub_path * count + i])
            .collect();
        let mut max_nesting_level = -1;
        let mut max_nesting_level_index = None;
        for &c in &candidates {
            let nesting_level = candidates
                .iter()
                .filter(|&&o| is_inside[c * count + o])
                .count() as i32;
            if nesting_level > max_nesting_level {
                max_nesting_level = nesting_level;
                max_nesting_level_index = Some(c);
            }
        }
        max_nesting_level_index
    };

    let mut path_changed = false;
    // A default QQuadPath with the hints of `path`: its fill rule is odd-even.
    let mut fixed_path = QuadPath {
        hints: path.hints,
        to_start: true,
        ..Default::default()
    };
    for i in 0..count {
        match find_closest_outer_subpath(i) {
            Some(j) if is_filled[i] == is_filled[j] => path_changed = true,
            _ => {
                for k in starts[i]..=ends[i] {
                    fixed_path.add_element(path.el[k as usize]);
                }
            }
        }
    }
    if path_changed {
        *path = fixed_path;
    }
    path_changed
}

#[derive(Clone, Copy)]
struct IntersectionData {
    e1: i32,
    e2: i32,
    t1: f32,
    t2: f32,
    in1: bool,
    in2: bool,
    out1: bool,
    out2: bool,
}

impl IntersectionData {
    /// The path through element `i` was taken, moving forward or backward.
    fn mark_handled(&mut self, i: i32, forward: bool) {
        if self.e1 == i {
            if forward {
                self.out1 = true;
            } else {
                self.in1 = true;
            }
        } else if self.e2 == i {
            if forward {
                self.out2 = true;
            } else {
                self.in2 = true;
            }
        }
    }
    fn all_handled(&self) -> bool {
        self.in1 && self.in2 && self.out1 && self.out2
    }
}

/// QSGCurveProcessor::solveIntersections: a path that crosses itself becomes one that does not
/// and fills the same area. True if the path changed.
fn solve_intersections(path: &mut QuadPath) -> bool {
    if path.hints & PATH_NON_INTERSECTING != 0 {
        return remove_nested_subpaths(path);
    }
    if path.el.len() < 2 {
        path.hints |= PATH_NON_INTERSECTING;
        return false;
    }

    let mut intersections: Vec<IntersectionData> = Vec::new();
    for (e1, e2) in find_overlapping_candidates(path) {
        let mut res = Vec::new();
        if is_intersecting(path, e1, e2, &mut res) {
            for (t1, t2) in res {
                intersections.push(IntersectionData {
                    e1,
                    e2,
                    t1,
                    t2,
                    in1: false,
                    in2: false,
                    out1: false,
                    out2: false,
                });
            }
        }
    }
    if intersections.is_empty() {
        path.hints |= PATH_NON_INTERSECTING;
        return remove_nested_subpaths(path);
    }

    let (starts, ends) = sub_path_bounds(path);
    let mut sub_path_handled = vec![false; ends.len()];
    let sub_path_index = |index: i32| -> usize {
        (0..starts.len())
            .find(|&i| index >= starts[i] && index <= ends[i])
            .unwrap_or(0)
    };
    // The element and fraction next to (i, t) when t left [0, 1].
    let ensure_in_bounds = |path: &QuadPath, i: &mut i32, t: &mut f32, delta_t: f32| {
        if *t <= 0.0 {
            if path.el[*i as usize].sub_start {
                *i = ends[sub_path_index(*i)];
            } else {
                *i -= 1;
            }
            *t = 1.0 - delta_t;
        } else if *t >= 1.0 {
            if path.el[*i as usize].sub_end {
                *i = starts[sub_path_index(*i)];
            } else {
                *i += 1;
            }
            *t = delta_t;
        }
    };
    // A start is where the fill is on one side only: on the right forward, on the left backward.
    let find_start = |path: &QuadPath, start: i32, end: i32| -> Option<(i32, bool)> {
        for i in start..end {
            let adjacent = match starts.iter().position(|&s| s == i) {
                Some(p) => ends[p],
                None => i - 1,
            };
            match path.fill_side_of(i, 1e-4) {
                FillSide::Right => return Some((i, true)),
                FillSide::Left => return Some((adjacent, false)),
                _ => {}
            }
        }
        None
    };

    let mut handled_elements = vec![false; path.el.len()];
    let mut regular_visit = true;

    let mut fixed_path = QuadPath {
        winding: path.winding,
        to_start: true,
        ..Default::default()
    };

    let mut i1;
    let mut t1;
    let mut i2 = 0;
    let mut t2 = 0.0f32;
    let mut t;
    let mut forward;
    let mut started_at_index;
    let mut started_at_t;
    match find_start(path, 0, path.el.len() as i32) {
        Some((i, f)) => {
            i1 = i;
            forward = f;
        }
        None => return false,
    }

    macro_rules! start_new_sub_path {
        ($i:expr, $forward:expr) => {{
            if $forward {
                fixed_path.move_to(path.el[$i as usize].sp);
                t = 0.0;
            } else {
                fixed_path.move_to(path.el[$i as usize].ep);
                t = 1.0;
            }
            started_at_t = t;
            started_at_index = $i;
            sub_path_handled[sub_path_index($i)] = true;
        }};
    }
    start_new_sub_path!(i1, forward);

    let mut total_iterations = 0;
    let mut prev_intersection: i32 = -1;
    let sign = |forward: bool| if forward { 1.0f32 } else { -1.0 };

    loop {
        // A corner point is not processed twice.
        if regular_visit && (t == 0.0 || t == 1.0) {
            let mut next_index = i1;
            if t == 1.0 && path.el[i1 as usize].sub_end {
                next_index = starts[sub_path_index(i1)];
            } else if t == 1.0 {
                next_index += 1;
            }
            if handled_elements[next_index as usize] {
                return false;
            }
            handled_elements[next_index as usize] = true;
        }
        total_iterations += 1;

        // The next intersection in the direction of travel that is closest to t.
        let mut ic: i32 = -1;
        let sg = sign(forward);
        t1 = sg;
        for (j, it) in intersections.iter().enumerate() {
            if j as i32 == prev_intersection {
                continue;
            }
            if i1 == it.e1 && it.t1 * sg >= t * sg && it.t1 * sg < t1 * sg {
                ic = j as i32;
                t1 = it.t1;
                i2 = it.e2;
                t2 = it.t2;
            }
            if i1 == it.e2 && it.t2 * sg >= t * sg && it.t2 * sg < t1 * sg {
                ic = j as i32;
                t1 = it.t2;
                i2 = it.e1;
                t2 = it.t1;
            }
        }
        prev_intersection = ic;

        if ic < 0 {
            regular_visit = true;
            // No intersection on this element: the rest of it goes to the fixed path.
            let e = path.el[i1 as usize];
            if forward {
                if e.line {
                    fixed_path.line_to(e.ep);
                } else {
                    let rest = e.segment_from_to(t, 1.0);
                    fixed_path.quad_to(rest.cp, rest.ep);
                }
                if e.sub_end {
                    let index = ends.iter().position(|&x| x == i1).unwrap_or(0);
                    i1 = starts[index];
                } else {
                    i1 += 1;
                }
                t = 0.0;
            } else {
                if e.line {
                    fixed_path.line_to(e.sp);
                } else {
                    let rest = e.segment_from_to(0.0, t).reversed();
                    fixed_path.quad_to(rest.cp, rest.ep);
                }
                if e.sub_start {
                    let index = starts.iter().position(|&x| x == i1).unwrap_or(0);
                    i1 = ends[index];
                } else {
                    i1 -= 1;
                }
                t = 1.0;
            }
        } else {
            let icu = ic as usize;
            sub_path_handled[sub_path_index(i2)] = true;
            // The path that led here is handled; we moved towards the intersection.
            intersections[icu].mark_handled(i1, !forward);

            let elem1 = path.el[i1 as usize];
            if elem1.line {
                fixed_path.line_to(elem1.point_at_fraction(t1));
            } else {
                let part = if forward {
                    elem1.segment_from_to(t, t1)
                } else {
                    elem1.segment_from_to(t1, t).reversed()
                };
                fixed_path.quad_to(part.cp, part.ep);
            }

            let it = intersections[icu];
            if it.in1 && it.in2 && it.out1 && !it.out2 {
                i1 = it.e2;
                t = it.t2;
                forward = true;
            } else if it.in1 && it.in2 && !it.out1 && it.out2 {
                i1 = it.e1;
                t = it.t1;
                forward = true;
            } else if it.in1 && !it.in2 && it.out1 && it.out2 {
                i1 = it.e2;
                t = it.t2;
                forward = false;
            } else if !it.in1 && it.in2 && it.out1 && it.out2 {
                i1 = it.e1;
                t = it.t1;
                forward = false;
            } else if t1 != 0.0 && t1 != 1.0 && t2 != 0.0 && t2 != 1.0 {
                // The angle of the crossing decides: a winding fill takes the leftmost path so
                // the inside stays on the right, an odd-even fill the rightmost.
                let mut tangent1 = elem1.tangent_at_fraction(t1);
                if !forward {
                    tangent1 = neg(tangent1);
                }
                let tangent2 = path.el[i2 as usize].tangent_at_fraction(t2);
                let angle = angle_between(neg(tangent1), tangent2);
                const DELTA_ANGLE: f32 = 1e-3;
                if (angle > DELTA_ANGLE && path.winding) || (angle < -DELTA_ANGLE && !path.winding)
                {
                    forward = true;
                    i1 = i2;
                    t = t2;
                } else if (angle < -DELTA_ANGLE && path.winding)
                    || (angle > DELTA_ANGLE && !path.winding)
                {
                    forward = false;
                    i1 = i2;
                    t = t2;
                }
                // Otherwise the curves only touch: the path and its direction stay.
            } else {
                // An intersection at a corner: the path a little further on decides.
                const DELTA_T: f32 = 1e-4;
                let mut i2_after = i2;
                let mut t2_after = t2 + DELTA_T;
                ensure_in_bounds(path, &mut i2_after, &mut t2_after, DELTA_T);
                if path.fill_side_of(i2_after, t2_after) == FillSide::Right {
                    forward = true;
                    i1 = i2;
                    t = t2;
                } else {
                    let mut i2_before = i2;
                    let mut t2_before = t2 - DELTA_T;
                    ensure_in_bounds(path, &mut i2_before, &mut t2_before, DELTA_T);
                    if path.fill_side_of(i2_before, t2_before) == FillSide::Left {
                        forward = false;
                        i1 = i2;
                        t = t2;
                    }
                }
            }

            // The path that leaves the intersection is handled too.
            if !(i1 == started_at_index && t == started_at_t) {
                intersections[icu].mark_handled(i1, forward);
            }
            if intersections[icu].all_handled() {
                intersections.remove(icu);
                prev_intersection = -1;
            }
            regular_visit = false;
        }

        if i1 == started_at_index && t == started_at_t {
            // This subpath is done: the next one is an unhandled subpath, or one that the
            // intersections cut out.
            let mut next_unhandled = false;
            for i in 0..sub_path_handled.len() {
                if !sub_path_handled[i] {
                    sub_path_handled[i] = true;
                    if let Some((s, f)) = find_start(path, starts[i], ends[i]) {
                        i1 = s;
                        forward = f;
                        next_unhandled = true;
                        start_new_sub_path!(i1, forward);
                        regular_visit = true;
                        break;
                    }
                }
            }

            if !next_unhandled {
                loop {
                    if intersections.is_empty() {
                        fixed_path.hints |= PATH_NON_INTERSECTING;
                        *path = fixed_path;
                        return true;
                    }
                    prev_intersection = 0;
                    regular_visit = false;
                    let un = intersections[0];
                    // A direction is good when the path just past the intersection has the fill on
                    // one side only.
                    let mut found = false;
                    for (k, (handled, next_e, next_t, next_forward)) in [
                        (un.in1, un.e1, un.t1, false),
                        (un.in2, un.e2, un.t2, false),
                        (un.out1, un.e1, un.t1, true),
                        (un.out2, un.e2, un.t2, true),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        if handled {
                            continue;
                        }
                        const DELTA_T: f32 = 1e-4;
                        let mut e_forward = next_e;
                        let mut t_forward = next_t + if next_forward { DELTA_T } else { -DELTA_T };
                        ensure_in_bounds(path, &mut e_forward, &mut t_forward, DELTA_T);
                        let fill_side = path.fill_side_of(e_forward, t_forward);
                        if (next_forward && fill_side == FillSide::Right)
                            || (!next_forward && fill_side == FillSide::Left)
                        {
                            fixed_path.move_to(path.el[next_e as usize].point_at_fraction(next_t));
                            i1 = next_e;
                            started_at_index = next_e;
                            t = next_t;
                            started_at_t = next_t;
                            forward = next_forward;
                            let d = &mut intersections[0];
                            match k {
                                0 => d.in1 = true,
                                1 => d.in2 = true,
                                2 => d.out1 = true,
                                _ => d.out2 = true,
                            }
                            found = true;
                            break;
                        }
                    }
                    if found {
                        break;
                    }
                    intersections.remove(0);
                }
            }
        }

        if total_iterations >= path.el.len() * 50 {
            break;
        }
    }
    false
}

// ---------------------------------------------------------------- QSGCurveProcessor::processFill

/// One triangle of a QSGCurveFillNode: corners, their normals, the curve coordinates
/// (u, v, kind) at each corner and what one unit of x and of y adds to (u, v).
#[derive(Clone, Copy, Debug)]
pub struct FillTri {
    pub v: [V; 3],
    pub n: [V; 3],
    pub uv: [[f32; 3]; 3],
    pub duvdx: V,
    pub duvdy: V,
}

/// (0, 0) maps to uv (0, 0), `v1` to (1, 0) and `v2` to (0, 1).
fn uv_for_point(v1: V, v2: V, p: V) -> V {
    let divisor = (v1[0] * v2[1] - v2[0] * v1[1]) as f64;
    let u = ((p[0] * v2[1] - p[1] * v2[0]) as f64 / divisor) as f32;
    let v = ((p[1] * v1[0] - p[0] * v1[1]) as f64 / divisor) as f32;
    [u, v]
}

/// uv of `p` for the quadratic curve from `p0` to `p2` with control point `p1`.
fn curve_uv(p0: V, p1: V, p2: V, p: V) -> V {
    let v1 = mul(sub(p1, p0), 2.0);
    let v2 = sub(sub(p2, v1), p0);
    uv_for_point(v1, v2, sub(p, p0))
}

fn element_uv_for_point(e: &QElem, p: V) -> [f32; 3] {
    let uv = curve_uv(e.sp, e.reference_point(), e.ep, p);
    let z = if e.line {
        0.0
    } else if e.is_convex() {
        -1.0
    } else {
        1.0
    };
    [uv[0], uv[1], z]
}

/// qRound64(float)
fn q_round64(f: f32) -> i64 {
    if f >= 0.0 {
        (f + 0.5) as i64
    } else {
        (f - 0.5) as i64
    }
}

fn round_vec2d(p: V) -> V {
    [
        q_round64(p[0] * 32.0) as f32 / 32.0,
        q_round64(p[1] * 32.0) as f32 / 32.0,
    ]
}

/// QVector2D as a hash key: -0 is 0.
fn key(p: V) -> (u32, u32) {
    ((p[0] + 0.0).to_bits(), (p[1] + 0.0).to_bits())
}

/// QSGCurveFillNode::appendTriangle; `el` None is a triangle inside the fill.
fn append_triangle(out: &mut Vec<FillTri>, el: Option<&QElem>, v: [V; 3], n: [V; 3]) {
    let uv_at = |p: V| match el {
        Some(e) => element_uv_for_point(e, p),
        None => [0.0, 1.0, -1.0],
    };
    let uv = [uv_at(v[0]), uv_at(v[1]), uv_at(v[2])];
    let dx = uv_at(add(v[0], [1.0, 0.0]));
    let dy = uv_at(add(v[0], [0.0, 1.0]));
    out.push(FillTri {
        v,
        n,
        uv,
        duvdx: [dx[0] - uv[0][0], dx[1] - uv[0][1]],
        duvdy: [dy[0] - uv[0][0], dy[1] - uv[0][1]],
    });
}

/// The normal of `start`-`end` that points away from `inside`.
fn outside_normal(start: V, end: V, inside: V) -> V {
    let normal = normal_of(sub(end, start));
    if dot(sub(inside, start), normal) < 0.0 {
        normal
    } else {
        neg(normal)
    }
}

fn add_triangle_for_line(out: &mut Vec<FillTri>, e: &QElem, sp: V, ep: V, cp: V) {
    append_triangle(out, Some(e), [sp, cp, ep], [Z; 3]);
    // Triangles on the outer side make room for antialiasing.
    let normal = outside_normal(sp, ep, cp);
    append_triangle(out, Some(e), [sp, sp, ep], [Z, normal, Z]);
    append_triangle(out, Some(e), [sp, ep, ep], [normal, normal, Z]);
}

fn add_triangle_for_convex(out: &mut Vec<FillTri>, e: &QElem, sp: V, ep: V, cp: V) {
    append_triangle(out, Some(e), [sp, cp, ep], [Z; 3]);
    let normal = outside_normal(sp, cp, ep);
    append_triangle(out, Some(e), [sp, sp, cp], [Z, normal, Z]);
    let normal = outside_normal(ep, cp, sp);
    append_triangle(out, Some(e), [ep, ep, cp], [Z, normal, Z]);
}

fn point_in_safe_space(p: V, e: &QElem) -> bool {
    let (a, b, c) = (e.sp, e.ep, e.cp);
    // The curve never crosses the line through A and B mirrored through C, nor the one
    // through B and A mirrored through C.
    let n1 = calc_normal(a, add(c, sub(c, b)));
    let n2 = calc_normal(b, add(c, sub(c, a)));
    let same_side = |p1: V, p2: V, line_point: V, n: V| {
        dot(sub(p1, line_point), n) * dot(sub(p2, line_point), n) >= 0.0
    };
    same_side(p, c, a, n1) && same_side(p, c, b, n2)
}

fn process_fill(path: &QuadPath) -> Vec<FillTri> {
    let mut out = Vec::new();
    let mut hull = LinePath::default();
    // QMultiHash: the values of one key come back newest first.
    let mut point_hash: HashMap<(u32, u32), Vec<i32>> = HashMap::new();

    for index in path.leaves() {
        let e = path.at(index);
        let rsp = round_vec2d(e.sp);
        if e.sub_start {
            hull.move_to(e.sp[0] as f64, e.sp[1] as f64);
        }
        if e.line {
            hull.line_to(e.ep[0] as f64, e.ep[1] as f64);
            point_hash.entry(key(rsp)).or_default().push(index);
        } else {
            let rep = round_vec2d(e.ep);
            let rcp = round_vec2d(e.cp);
            if e.is_convex() {
                hull.line_to(e.ep[0] as f64, e.ep[1] as f64);
                add_triangle_for_convex(&mut out, e, rsp, rep, rcp);
                point_hash.entry(key(rsp)).or_default().push(index);
            } else {
                hull.line_to(e.cp[0] as f64, e.cp[1] as f64);
                hull.line_to(e.ep[0] as f64, e.ep[1] as f64);
                add_triangle_for_line(&mut out, e, rsp, rep, rcp);
                point_hash.entry(key(rcp)).or_default().push(index);
            }
        }
    }

    // False if the triangle lies on more than one element and has to be split.
    let handle_triangle = |out: &mut Vec<FillTri>, p: &[V; 3]| -> bool {
        let mut found: Option<(usize, usize, i32)> = None;
        for i in 0..3 {
            let Some(range) = point_hash.get(&key(round_vec2d(p[i]))) else {
                continue;
            };
            let mut test_index = *range.last().unwrap();
            if range.len() > 1 {
                // The triangle is on the inside of exactly one of the elements.
                for &it in range.iter().rev() {
                    let el = path.at(it);
                    let fill_on_left = !el.is_fill_on_right();
                    let sp = round_vec2d(el.sp);
                    let ep = round_vec2d(el.ep);
                    let point_inside = |q: V| {
                        q == sp || q == ep || is_point_on_left(q, el.sp, el.ep) == fill_on_left
                    };
                    if point_inside(p[0]) && point_inside(p[1]) && point_inside(p[2]) {
                        test_index = it;
                        break;
                    }
                }
            }
            let element = path.at(test_index);
            // Is p[i]-p[j] on the element for some j: sp-ep for a line or a convex curve,
            // cp-sp or cp-ep for a concave one.
            for (j, &pj) in p.iter().enumerate() {
                if i == j {
                    continue;
                }
                let on_element = if element.is_convex() || element.line {
                    round_vec2d(element.ep) == pj
                } else {
                    round_vec2d(element.sp) == pj || round_vec2d(element.ep) == pj
                };
                if on_element {
                    if found.is_some() {
                        return false;
                    }
                    found = Some((i, j, test_index));
                    break;
                }
            }
        }

        let Some((si, ei, element_index)) = found else {
            append_triangle(out, None, *p, [Z; 3]);
            return true;
        };
        let element = path.at(element_index);
        let oi = 3 - si - ei;
        if element.line {
            add_triangle_for_line(out, element, p[si], p[ei], p[oi]);
        } else if !element.is_convex() || point_in_safe_space(p[oi], element) {
            append_triangle(out, Some(element), *p, [Z; 3]);
        } else {
            // A point inside the triangle that is in the safe space too.
            let mut new_point = [
                (p[0][0] + p[1][0] + p[2][0]) / 3.0,
                (p[0][1] + p[1][1] + p[2][1]) / 3.0,
            ];
            let mut safe_space = false;
            for _ in 0..7 {
                safe_space = point_in_safe_space(new_point, element);
                if safe_space {
                    break;
                }
                let s = add(add(p[si], p[ei]), new_point);
                new_point = [s[0] / 3.0, s[1] / 3.0];
            }
            if safe_space {
                append_triangle(out, Some(element), [p[si], p[ei], new_point], [Z; 3]);
                append_triangle(out, None, [p[si], p[oi], new_point], [Z; 3]);
                append_triangle(out, None, [p[ei], p[oi], new_point], [Z; 3]);
            } else {
                append_triangle(out, None, *p, [Z; 3]);
            }
        }
        true
    };

    let (vertices, mut indices) = triangulate(&hull, path.winding);
    // Qt's workaround for qTriangulate() on a path of one triangle.
    if indices.len() == 3 {
        indices = vec![0, 1, 2];
    }
    for idx in indices.as_chunks::<3>().0 {
        let mut p: [V; 3] = std::array::from_fn(|i| {
            round_vec2d([
                vertices[idx[i] as usize * 2] as f32,
                vertices[idx[i] as usize * 2 + 1] as f32,
            ])
        });
        if fnull_f(det3(p[0], p[1], p[2])) {
            continue;
        }
        if !handle_triangle(&mut out, &p) {
            let mut c = [
                (p[0][0] + p[1][0] + p[2][0]) / 3.0,
                (p[0][1] + p[1][1] + p[2][1]) / 3.0,
            ];
            for i in 0..3 {
                std::mem::swap(&mut c, &mut p[i]);
                handle_triangle(&mut out, &p);
                std::mem::swap(&mut c, &mut p[i]);
            }
        }
    }
    out
}

/// The triangles Qt's CurveRenderer fills a path with (QQuickShapeCurveRenderer::processPath).
/// `hints` is ShapePath.pathHints.
pub fn fill_tris(cmds: &[PathCmd], start: (f64, f64), even_odd: bool, hints: u32) -> Vec<FillTri> {
    let pp = build_painter_path(cmds, [start.0, start.1]);
    let mut q = QuadPath::from_painter_path(&pp, !even_odd, hints);
    q.to_start = true;
    let mut fill_path = q.sub_paths_closed();
    solve_intersections(&mut fill_path);
    fill_path.add_curvature_data();
    solve_overlaps(&mut fill_path);
    process_fill(&fill_path)
}

/// shapecurve.vert for the corners of `t` under `matrix_scale`: the antialiasing offset of
/// each, in the item's units, and its qt_TexCoord: u, v, the kind of the triangle and the
/// "outside of a concave curve" flag.
fn corners(t: &FillTri, matrix_scale: f32) -> [(V, [f32; 4]); 3] {
    const SQRT2: f32 = std::f32::consts::SQRT_2;
    let (dudx, dvdx, dudy, dvdy) = (t.duvdx[0], t.duvdx[1], t.duvdy[0], t.duvdy[1]);
    let z = t.uv[0][2];
    std::array::from_fn(|i| {
        let off = t.n[i].map(|n| n * SQRT2 / matrix_scale);
        let special = (z > 0.0) as i32 * ((off[0] != 0.0) as i32 + (off[1] != 0.0) as i32);
        let tc = [
            t.uv[i][0] + (off[0] * dudx + off[1] * dudy),
            t.uv[i][1] + (off[0] * dvdx + off[1] * dvdy),
            z,
            special as f32,
        ];
        (off, tc)
    })
}

/// What shapecurve.vert hands the rasterizer for `tris`, as Qt draws the node of an item that
/// is only moved: the item at `at` of the rect `over` (x y w h, scene units) Qt's projection
/// is over, on a target whose devicePixelRatio is `dpr`. Ten floats a vertex, three vertices a
/// triangle in Qt's order: gl_Position x y, qt_TexCoord, texCoordDerivative. The triangles
/// stay those of the unscaled path: the node's indices are 32 bits wide, which keeps it out of
/// a merged batch, so the shader's matrix is the projection times the item's translation and
/// `matrixScale` is the devicePixelRatio, which makes the antialiasing margin one pixel.
pub fn fill_vertices(tris: &[FillTri], over: [f64; 4], at: [f32; 2], dpr: f32) -> Vec<f32> {
    let [m00, m11, m30, m31] = crate::raster::ortho(over);
    // QMatrix4x4's product of the projection and the translation.
    let (m30, m31) = (m00 * at[0] + m30, m11 * at[1] + m31);
    let mut out = Vec::with_capacity(tris.len() * 30);
    for t in tris {
        let d = [t.duvdx[0], t.duvdx[1], t.duvdy[0], t.duvdy[1]].map(|d| d / dpr);
        for (i, (off, tc)) in corners(t, dpr).into_iter().enumerate() {
            let p = add(t.v[i], off);
            out.extend_from_slice(&[m00 * p[0] + m30, m11 * p[1] + m31]);
            out.extend_from_slice(&tc);
            out.extend_from_slice(&d);
        }
    }
    out
}

/// The render target a fill is drawn in, as Qt's renderer has it: the rect (x y w h) its
/// projection is over, its size in pixels, and where in the rect's units the item's corner is.
pub struct FillFrame {
    pub over: [f64; 4],
    pub pixels: [f32; 2],
    pub at: [f32; 2],
}

/// shapecurve.vert and shapecurve.frag for a solid colour on the processor, for a target that
/// is not the graphics card's: the triangles, in the item's units, each blended premultiplied
/// source-over. The item's unit is `scale` pixels and its corner is at pixel `at` of `cv`: the
/// triangles stay those of the unscaled path, as Qt's, and the vertex shader's `matrixScale`
/// makes the antialiasing margin one pixel. `origin` is where pixel (0, 0) of `cv` is in the
/// render target: screen-space derivatives are taken in 2 x 2 blocks of the target's pixels.
/// `in_texture`: the target is a layer's texture, not the window. `frame`, where it is known:
/// the render target as Qt projects onto it, which places a vertex to the last bit.
#[allow(clippy::too_many_arguments)]
pub fn draw_fill(
    cv: &mut Canvas,
    tris: &[FillTri],
    color: [u8; 4],
    scale: [f32; 2],
    at: [f32; 2],
    frame: Option<&FillFrame>,
    opacity: f32,
    origin: (i64, i64),
    in_texture: bool,
) {
    // QMatrix4x4's product of the projection and the item's translation, as `fill_vertices`.
    let projection = frame.map(|f| {
        let [m00, m11, m30, m31] = crate::raster::ortho(f.over);
        [m00, m11, m00 * f.at[0] + m30, m11 * f.at[1] + m31]
    });
    // QSGCurveFillMaterialShader::updateUniformData: sqrt(|determinant|) * devicePixelRatio.
    let matrix_scale = (scale[0] * scale[1]).abs().sqrt();
    let ca = color[3] as f32 / 255.0;
    let col = [
        color[0] as f32 / 255.0 * ca,
        color[1] as f32 / 255.0 * ca,
        color[2] as f32 / 255.0 * ca,
        ca,
    ];
    // lp_setup_tri.c's subpixel_snap of the position less the half pixel offset, put back as
    // the 128 the pixel centres stand at.
    let snap = |v: f32| ((v - 0.5) * 256.0).round_ties_even() as i64 + 128;
    let fma = crate::raster::fma;
    for t in tris {
        // Per corner: where it is in pixels, and its qt_TexCoord.
        let cs = corners(t, matrix_scale);
        let fp: [V; 3] = std::array::from_fn(|i| {
            let p = add(t.v[i], cs[i].0);
            match (projection, frame) {
                // The matrix, then the viewport, each one fused step as the rasterizer has
                // them; `origin` is where `cv` starts in the frame.
                (Some([m00, m11, m30, m31]), Some(f)) => {
                    let (hw, hh) = (f.pixels[0] * 0.5, f.pixels[1] * 0.5);
                    let clip = [fma(m00, p[0], m30), fma(m11, p[1], m31)];
                    [
                        fma(clip[0], hw, hw) - origin.0 as f32,
                        fma(clip[1], -hh, hh) - origin.1 as f32,
                    ]
                }
                _ => [p[0] * scale[0] + at[0], p[1] * scale[1] + at[1]],
            }
        });
        let tc = cs.map(|c| c.1);
        if fp.iter().flatten().any(|c| !c.is_finite()) {
            continue;
        }
        // Which pixels are inside is decided on the 1/256 grid in whole numbers. A pixel centre
        // exactly on an edge belongs to the triangle below it in a texture and to the one above
        // it in a window, whose rows GL counts from the bottom.
        let iv = fp.map(|p| [snap(p[0]), snap(p[1])]);
        let area = (iv[1][0] - iv[0][0]) * (iv[2][1] - iv[0][1])
            - (iv[1][1] - iv[0][1]) * (iv[2][0] - iv[0][0]);
        if area == 0 {
            continue;
        }
        let iv = if area > 0 { iv } else { [iv[0], iv[2], iv[1]] };
        let inside = |a: [i64; 2], b: [i64; 2], p: [i64; 2]| {
            let e = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            e > 0 || (e == 0 && (dy < 0 || (dy == 0 && (dx > 0) == in_texture)))
        };
        // The plane of each of the four as llvmpipe sets it up (lp_state_setup.c, calc_coef4,
        // as `Canvas::tri_with` has it for colours): the value at the origin of pixel centres,
        // and its steps across and down.
        let (l0, l1, l2) = if area < 0 { (0, 1, 2) } else { (1, 0, 2) };
        let (dx01, dy01) = (fp[l0][0] - fp[l1][0], fp[l0][1] - fp[l1][1]);
        let (dx20, dy20) = (fp[l2][0] - fp[l0][0], fp[l2][1] - fp[l0][1]);
        let ooa = 1.0 / (dx01 * dy20 - dx20 * dy01);
        let (dx01, dy01, dx20, dy20) = (dx01 * ooa, dy01 * ooa, dx20 * ooa, dy20 * ooa);
        let (ox, oy) = (fp[l0][0] - 0.5, fp[l0][1] - 0.5);
        let planes: [[f32; 3]; 4] = std::array::from_fn(|k| {
            let (da01, da20) = (tc[l0][k] - tc[l1][k], tc[l2][k] - tc[l0][k]);
            let dadx = da01 * dy20 - da20 * dy01;
            let dady = da20 * dx01 - da01 * dx20;
            [tc[l0][k] - (dadx * ox + dady * oy), dadx, dady]
        });
        let lo = |k: usize| iv.iter().map(|p| p[k]).min().unwrap() >> 8;
        let hi = |k: usize| (iv.iter().map(|p| p[k]).max().unwrap() >> 8) + 1;
        let x0 = lo(0).max(0);
        let x1 = hi(0).min(cv.w as i64 - 1);
        let y0 = lo(1).max(0);
        let y1 = hi(1).min(cv.h as i64 - 1);
        for py in y0..=y1 {
            for px in x0..=x1 {
                let ip = [px * 256 + 128, py * 256 + 128];
                if !(inside(iv[1], iv[2], ip)
                    && inside(iv[2], iv[0], ip)
                    && inside(iv[0], iv[1], ip))
                {
                    continue;
                }
                // qt_TexCoord at a pixel, also outside the triangle (lp_bld_interp.c,
                // interp_attrib_linear: two fused multiply-adds), and the shader's `f` there.
                let varying = |k: usize, x: i64, y: i64| {
                    let [at0, dadx, dady] = planes[k];
                    fma(dady, y as f32, fma(dadx, x as f32, at0))
                };
                let f_at = |x: i64, y: i64| {
                    let (u, v, z) = (varying(0, x, y), varying(1, x, y), varying(2, x, y));
                    z * (u * u - v) + (1.0 - z.abs()) * (u - v)
                };
                let (u, sw) = (varying(0, px, py), varying(3, px, py));
                let f = f_at(px, py);
                // dFdx and dFdy as the rasterizer has them: differences inside the 2 x 2 block
                // of the target's pixels the pixel is in, along its row and along its column.
                let qx = ((px + origin.0) & !1) - origin.0;
                let qy = ((py + origin.1) & !1) - origin.1;
                let dfx = f_at(qx + 1, py) - f_at(qx, py);
                let dfy = f_at(px, qy + 1) - f_at(px, qy);
                let df = (dfx * dfx + dfy * dfy).sqrt();
                // The outer triangles of a concave curve are masked beyond the curve's ends.
                let mask = 1 - (sw != 0.0) as i32 * ((u < 0.0) as i32 + (u > 1.0) as i32);
                let k = (0.5 + f / df).clamp(0.0, 1.0) * mask as f32;
                if k.is_nan() || k <= 0.0 {
                    continue;
                }
                cv.blend_f(px as usize, py as usize, col.map(|c| c * k * opacity));
            }
        }
    }
}
