//! qpathsimplifier.cpp (Qt 6.12) port: qSimplifyPath for the distance-field generator.
//! Points are 24.8 fixed (Q_FIXED_POINT_SCALE 256); output polygons separated by u32::MAX.

pub type P = (i32, i32);
pub const END: u32 = u32::MAX;
const SCALE: f64 = 256.0;

fn lt(a: P, b: P) -> bool {
    a.1 < b.1 || (a.1 == b.1 && a.0 < b.0)
}
fn sub(a: P, b: P) -> P {
    (a.0.wrapping_sub(b.0), a.1.wrapping_sub(b.1))
}
fn cross(u: P, v: P) -> i32 {
    u.0.wrapping_mul(v.1).wrapping_sub(u.1.wrapping_mul(v.0))
}
fn dot(u: P, v: P) -> i32 {
    u.0.wrapping_mul(v.0).wrapping_add(u.1.wrapping_mul(v.1))
}
/// qRound for qreal.
fn qround(v: f64) -> i32 {
    if v >= 0.0 {
        (v + 0.5).floor() as i32
    } else {
        (v - 0.5).ceil() as i32
    }
}

#[derive(Clone, Copy, Default)]
struct Fraction {
    num: u32,
    den: u32,
}
fn gcd(mut x: u32, mut y: u32) -> u32 {
    while y != 0 {
        let z = y;
        y = x % y;
        x = z;
    }
    x
}
fn fraction(n: u32, d: u32) -> Fraction {
    if n == 0 {
        Fraction { num: 0, den: 1 }
    } else {
        let g = gcd(n, d);
        Fraction {
            num: n / g,
            den: d / g,
        }
    }
}
#[derive(Clone, Copy, Default)]
struct Rational {
    int: i32,
    frac: Fraction,
}
#[derive(Clone, Copy, Default)]
struct IPoint {
    x: Rational,
    y: Rational,
}
impl IPoint {
    fn valid(&self) -> bool {
        self.x.frac.den != 0 && self.y.frac.den != 0
    }
    fn accurate(&self) -> bool {
        self.x.frac.num == 0 && self.y.frac.num == 0
    }
    fn round(&self) -> P {
        let mut r = (self.x.int, self.y.int);
        if 2 * self.x.frac.num as u64 >= self.x.frac.den as u64 {
            r.0 += 1;
        }
        if 2 * self.y.frac.num as u64 >= self.y.frac.den as u64 {
            r.1 += 1;
        }
        r
    }
}
fn point_side_of_line(p: P, v1: P, v2: P) -> i32 {
    let ux = v2.0 as i64 - v1.0 as i64;
    let uy = v2.1 as i64 - v1.1 as i64;
    let vx = p.0 as i64 - v1.0 as i64;
    let vy = p.1 as i64 - v1.1 as i64;
    let c = ux * vy - uy * vx;
    c.signum() as i32
}
fn intersection_point(u1: P, u2: P, v1: P, v2: P) -> IPoint {
    let mut r = IPoint {
        x: Rational {
            int: 0,
            frac: Fraction { num: 0, den: 0 },
        },
        y: Rational {
            int: 0,
            frac: Fraction { num: 0, den: 0 },
        },
    };
    let u = sub(u2, u1);
    let v = sub(v2, v1);
    let mut d1 = cross(u, sub(v1, u1));
    let mut d2 = cross(u, sub(v2, u1));
    let mut det = d2.wrapping_sub(d1);
    let mut d3 = cross(v, sub(u1, v1));
    let mut d4 = d3.wrapping_sub(det);
    if det == 0 {
        return r;
    }
    if det < 0 {
        det = -det;
        d1 = -d1;
        d2 = -d2;
        d3 = -d3;
        d4 = -d4;
    }
    if d1 >= 0 || d2 <= 0 || d3 <= 0 || d4 >= 0 {
        return r;
    }
    let comp = |vc: i32, a: i32, b: i32| -> Rational {
        let (base, d) = if vc >= 0 { (a, d1) } else { (b, d2) };
        let m = (-(vc as i64)) * d as i64;
        Rational {
            int: base.wrapping_add((m / det as i64) as i32),
            frac: fraction((m % det as i64) as u32, det as u32),
        }
    };
    r.x = comp(v.0, v1.0, v2.0);
    r.y = comp(v.1, v1.1, v2.1);
    r
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EvType {
    Upper = 0,
    Lower = 1,
}
#[derive(Clone, Copy)]
struct Event {
    point: P,
    ty: EvType,
    element: usize,
}
/// Event::operator<
fn ev_lt(a: &Event, b: &Event) -> bool {
    if a.point == b.point {
        return (a.ty as u8) < (b.ty as u8);
    }
    lt(b.point, a.point)
}

#[derive(Clone, Copy, Default)]
struct Element {
    middle: P,
    indices: [u32; 4],
    next: Option<usize>,
    previous: Option<usize>,
    winding: i32,
    /// edgeNode (connectElements) / bvhNode (removeIntersections) share a union in Qt.
    node: usize,
    degree: u8,
    processed: bool,
    pointing_up: bool,
    originally_pointing_up: bool,
}
impl Element {
    fn upper(&self) -> u32 {
        self.indices[if self.pointing_up {
            self.degree as usize
        } else {
            0
        }]
    }
    fn lower(&self) -> u32 {
        self.indices[if self.pointing_up {
            0
        } else {
            self.degree as usize
        }]
    }
    fn flip(&mut self) {
        let d = self.degree as usize;
        for i in 0..(d + 1) >> 1 {
            self.indices.swap(i, d - i);
        }
        self.pointing_up = !self.pointing_up;
    }
}

/// QPainterPath element (MoveTo / LineTo / CurveTo with its two CurveToData points).
#[derive(Clone, Copy, Debug)]
pub enum PathEl {
    Move(f64, f64),
    Line(f64, f64),
    Cubic(f64, f64, f64, f64, f64, f64),
}

pub struct Simplifier {
    elements: Vec<Element>,
    /// m_elements: element ids in Qt's order (split elements are appended).
    order: Vec<usize>,
    points: Vec<P>,
    indices: Vec<u32>,
    /// QRBTree<Element *> m_elementList as an ordered list of element ids.
    list: Vec<usize>,
    bvh: Vec<BvhNode>,
    bvh_root: usize,
}

#[derive(Clone, Copy)]
struct BvhNode {
    leaf: bool,
    min: P,
    max: P,
    /// Leaf: element id; split: left node.
    a: usize,
    /// Split: right node.
    b: usize,
}

/// qSimplifyPath(path, vertices, indices, matrix) with matrix = translate(t) * scale(s).
pub fn simplify(path: &[PathEl], s: f64, t: f64) -> (Vec<P>, Vec<u32>) {
    let mut sp = Simplifier {
        elements: vec![],
        order: vec![],
        points: vec![],
        indices: vec![],
        list: vec![],
        bvh: vec![],
        bvh_root: usize::MAX,
    };
    let map = |x: f64, y: f64| (x * s + t, y * s + t);
    sp.init_elements(path, &map);
    let mut ok = true;
    if !sp.order.is_empty() {
        sp.remove_intersections();
        ok = sp.connect_elements();
        if ok {
            sp.fill_indices();
        }
    }
    if !ok {
        return (vec![], vec![]);
    }
    (sp.points, sp.indices)
}

impl Simplifier {
    fn pt(&self, i: u32) -> P {
        self.points[i as usize]
    }
    fn new_element(&mut self) -> usize {
        self.elements.push(Element::default());
        self.elements.len() - 1
    }
    fn add_line(&mut self, from_i: u32, to_i: u32) {
        let (from, to) = (self.pt(from_i), self.pt(to_i));
        let e = self.new_element();
        let el = &mut self.elements[e];
        el.degree = 1;
        el.indices[0] = from_i;
        el.indices[1] = to_i;
        el.middle = ((from.0 + to.0) >> 1, (from.1 + to.1) >> 1);
        self.order.push(e);
    }
    fn fix(v: (f64, f64)) -> P {
        (qround(v.0 * SCALE), qround(v.1 * SCALE))
    }

    fn init_elements(&mut self, path: &[PathEl], map: &dyn Fn(f64, f64) -> (f64, f64)) {
        if path.is_empty() {
            return;
        }
        let (mut move_to, mut prev) = (0u32, 0u32);
        // Untransformed current point (p[-2], p[-1]).
        let mut last = (0.0f64, 0.0f64);
        for el in path {
            match *el {
                PathEl::Move(x, y) => {
                    if !self.points.is_empty() && self.pt(prev) != self.pt(move_to) {
                        self.add_line(prev, move_to);
                    }
                    prev = self.points.len() as u32;
                    move_to = prev;
                    self.points.push(Self::fix(map(x, y)));
                    last = (x, y);
                }
                PathEl::Line(x, y) => {
                    let to = Self::fix(map(x, y));
                    if to != *self.points.last().unwrap() {
                        let n = self.points.len() as u32;
                        self.points.push(to);
                        self.add_line(prev, n);
                        prev = n;
                    }
                    last = (x, y);
                }
                PathEl::Cubic(c1x, c1y, c2x, c2y, ex, ey) => {
                    let start = prev;
                    let end = Self::fix(map(ex, ey));
                    prev = self.points.len() as u32;
                    self.points.push(end);
                    let x1 = last.0 + 1.5 * (c1x - last.0);
                    let y1 = last.1 + 1.5 * (c1y - last.1);
                    let x2 = ex + 1.5 * (c2x - ex);
                    let y2 = ey + 1.5 * (c2y - ey);
                    let e = self.new_element();
                    if (x1 - x2).abs() < 1e-3 && (y1 - y2).abs() < 1e-3 {
                        let ctrl = Self::fix(map(x1, y1));
                        self.set_quadratic(e, start, ctrl, prev);
                    } else {
                        let c1 = Self::fix(map(c1x, c1y));
                        let c2 = Self::fix(map(c2x, c2y));
                        self.set_cubic_and_simplify(e, start, c1, c2, prev);
                    }
                    self.order.push(e);
                    last = (ex, ey);
                }
            }
        }
        if !self.points.is_empty() && self.pt(prev) != self.pt(move_to) {
            self.add_line(prev, move_to);
        }
        for &e in &self.order {
            self.elements[e].processed = false;
        }
    }

    // --- m_elementList (QRBTree) ---
    fn lpos(&self, e: usize) -> usize {
        self.list
            .iter()
            .position(|&x| x == e)
            .expect("edge in list")
    }
    fn lnext(&self, e: usize) -> Option<usize> {
        self.list.get(self.lpos(e) + 1).copied()
    }
    fn lprev(&self, e: usize) -> Option<usize> {
        let p = self.lpos(e);
        (p > 0).then(|| self.list[p - 1])
    }
    fn lfront(&self) -> Option<usize> {
        self.list.first().copied()
    }
    fn lback(&self) -> Option<usize> {
        self.list.last().copied()
    }
    /// attachAfter(left, node): left == null puts it first.
    fn lattach_after(&mut self, left: Option<usize>, e: usize) {
        let at = left.map_or(0, |l| self.lpos(l) + 1);
        self.list.insert(at, e);
    }
    fn ldelete(&mut self, e: usize) {
        let p = self.lpos(e);
        self.list.remove(p);
    }

    fn connect_elements(&mut self) -> bool {
        let mut events: Vec<Event> = Vec::with_capacity(self.order.len() * 2);
        for k in 0..self.order.len() {
            let e = self.order[k];
            let el = &mut self.elements[e];
            el.next = None;
            el.previous = None;
            el.winding = 0;
            let (u, v) = (
                self.points[el.indices[0] as usize],
                self.points[el.indices[el.degree as usize] as usize],
            );
            if u != v {
                el.pointing_up = lt(v, u);
                el.originally_pointing_up = el.pointing_up;
                let up = el.pointing_up;
                events.push(Event {
                    point: u,
                    ty: if up { EvType::Lower } else { EvType::Upper },
                    element: e,
                });
                events.push(Event {
                    point: v,
                    ty: if up { EvType::Upper } else { EvType::Lower },
                    element: e,
                });
            }
        }
        let mut ordered: Vec<usize> = vec![];
        if !events.is_empty() {
            sort_events(&mut events);
        }
        while let Some(&event) = events.last() {
            let ep = event.point;
            let bounds = self.outer_bounds(ep);
            let n = events.len();
            if event.ty == EvType::Lower && n > 2 {
                let first = match bounds.0 {
                    Some(b) => self.lnext(b),
                    None => self.lfront(),
                };
                let second = match bounds.1 {
                    Some(b) => self.lprev(b),
                    None => self.lback(),
                };
                let (e2, e3) = (events[n - 2], events[n - 3]);
                if first == second && e2.ty == EvType::Upper && e3.point != ep {
                    let (a, b) = (event.element, e2.element);
                    let p = self.lpos(a);
                    self.list[p] = b;
                    events.pop();
                    events.pop();
                    if self.elements[b].pointing_up != self.elements[a].pointing_up {
                        self.elements[b].flip();
                    }
                    self.elements[b].winding = self.elements[a].winding;
                    let mut w = self.elements[a].winding;
                    if self.elements[a].originally_pointing_up {
                        w += 1;
                    }
                    if w == 0 || w == 1 {
                        if self.elements[a].pointing_up {
                            self.elements[a].previous = Some(b);
                            self.elements[b].next = Some(a);
                        } else {
                            self.elements[a].next = Some(b);
                            self.elements[b].previous = Some(a);
                        }
                    }
                    continue;
                }
            }
            ordered.clear();
            if !self.list.is_empty() {
                let mut cur = match bounds.0 {
                    Some(b) => self.lnext(b),
                    None => self.lfront(),
                };
                while cur != bounds.1 {
                    let e = cur.unwrap();
                    let mut w = self.elements[e].winding;
                    if self.elements[e].originally_pointing_up {
                        w += 1;
                    }
                    let lower = self.pt(self.elements[e].lower());
                    if lower == ep {
                        if w == 0 || w == 1 {
                            ordered.push(e);
                        }
                    } else {
                        let ee = &self.elements[event.element];
                        let ii = if (event.ty == EvType::Upper) == ee.pointing_up {
                            ee.degree as usize
                        } else {
                            0
                        };
                        let pi = ee.indices[ii];
                        let ue = self.new_element();
                        self.elements[ue] = self.elements[e];
                        // upperElement->lowerIndex() = element->upperIndex() = pointIndex
                        {
                            let el = &mut self.elements[e];
                            let k = if el.pointing_up {
                                el.degree as usize
                            } else {
                                0
                            };
                            el.indices[k] = pi;
                        }
                        {
                            let el = &mut self.elements[ue];
                            let k = if el.pointing_up {
                                0
                            } else {
                                el.degree as usize
                            };
                            el.indices[k] = pi;
                        }
                        self.elements[e].next = None;
                        self.elements[e].previous = None;
                        if let Some(nx) = self.elements[ue].next {
                            self.elements[nx].previous = Some(ue);
                        } else if let Some(pv) = self.elements[ue].previous {
                            self.elements[pv].next = Some(ue);
                        }
                        if self.elements[e].pointing_up != self.elements[e].originally_pointing_up {
                            self.elements[e].flip();
                        }
                        if w == 0 || w == 1 {
                            ordered.push(ue);
                        }
                        self.order.push(ue);
                    }
                    cur = self.lnext(e);
                }
            }
            while let Some(&ev) = events.last() {
                if ev.point != ep {
                    break;
                }
                if ev.ty == EvType::Upper {
                    let left = self.find_element_left_of(ev.element, bounds);
                    self.lattach_after(left, ev.element);
                } else {
                    self.ldelete(ev.element);
                }
                events.pop();
            }
            if !self.list.is_empty() {
                let mut cur = match bounds.0 {
                    Some(b) => self.lnext(b),
                    None => self.lfront(),
                };
                let mut winding = bounds.0.map_or(0, |b| self.elements[b].winding);
                while cur != bounds.1 {
                    let e = cur.unwrap();
                    let mut ccw = winding & 1;
                    if self.elements[e].originally_pointing_up {
                        winding -= 1;
                    } else {
                        winding += 1;
                        ccw ^= 1;
                    }
                    self.elements[e].winding = winding;
                    if ccw == 0 {
                        self.elements[e].flip();
                    }
                    cur = self.lnext(e);
                }
                let mut cur = match bounds.1 {
                    Some(b) => self.lprev(b),
                    None => self.lback(),
                };
                while cur != bounds.0 {
                    let e = cur.unwrap();
                    let mut w = self.elements[e].winding;
                    if self.elements[e].originally_pointing_up {
                        w += 1;
                    }
                    if w == 0 || w == 1 {
                        ordered.push(e);
                    }
                    cur = self.lprev(e);
                }
            }
            if !ordered.is_empty() {
                if ordered.len() & 1 != 0 {
                    return false;
                }
                let mut i = 0;
                let f = ordered[0];
                if self.pt(self.elements[f].indices[0]) != ep {
                    ordered.push(f);
                    i = 1;
                }
                while i < ordered.len() {
                    let (nx, pv) = (ordered[i], ordered[i + 1]);
                    self.elements[nx].previous = Some(pv);
                    self.elements[pv].next = Some(nx);
                    i += 2;
                }
            }
        }
        true
    }

    fn fill_indices(&mut self) {
        for &e in &self.order {
            self.elements[e].processed = false;
        }
        for k in 0..self.order.len() {
            let start = self.order[k];
            let mut e = start;
            if self.elements[e].processed || self.elements[e].next.is_none() {
                continue;
            }
            loop {
                let el = self.elements[e];
                self.indices.push(el.indices[0]);
                match el.degree {
                    2 => {
                        let p = [
                            self.pt(el.indices[0]),
                            self.pt(el.indices[1]),
                            self.pt(el.indices[2]),
                        ];
                        self.sub_div_quadratic(p[0], p[1], p[2]);
                    }
                    3 => {
                        let p = [
                            self.pt(el.indices[0]),
                            self.pt(el.indices[1]),
                            self.pt(el.indices[2]),
                            self.pt(el.indices[3]),
                        ];
                        self.sub_div_cubic(p[0], p[1], p[2], p[3]);
                    }
                    _ => {}
                }
                self.elements[e].processed = true;
                e = el.next.unwrap();
                if e == start {
                    break;
                }
            }
            self.indices.push(END);
        }
    }

    fn new_bvh(&mut self, n: BvhNode) -> usize {
        self.bvh.push(n);
        self.bvh.len() - 1
    }

    fn remove_intersections(&mut self) {
        let mut els: Vec<usize> = self.order.clone();
        self.bvh.clear();
        self.bvh_root = self.build_tree(&mut els[..]);
        let mut stack: Vec<usize> = self.order.clone();
        while let Some(e) = stack.pop() {
            let node = self.elements[e].node;
            if !self.elements[e].processed && !self.intersect_nodes(&mut stack, node, self.bvh_root)
            {
                self.elements[e].processed = true;
            }
        }
        self.bvh.clear();
    }

    fn build_tree(&mut self, els: &mut [usize]) -> usize {
        let blank = BvhNode {
            leaf: true,
            min: (0, 0),
            max: (0, 0),
            a: 0,
            b: 0,
        };
        let node = self.new_bvh(blank);
        if els.len() == 1 {
            let e = els[0];
            self.elements[e].node = node;
            let el = self.elements[e];
            let mut mn = self.pt(el.indices[0]);
            let mut mx = mn;
            for i in 1..=el.degree as usize {
                let p = self.pt(el.indices[i]);
                mn = (mn.0.min(p.0), mn.1.min(p.1));
                mx = (mx.0.max(p.0), mx.1.max(p.1));
            }
            self.bvh[node] = BvhNode {
                leaf: true,
                min: mn,
                max: mx,
                a: e,
                b: 0,
            };
            return node;
        }
        let mid = |s: &Self, e: usize| s.elements[e].middle;
        let mut mn = mid(self, els[0]);
        let mut mx = mn;
        for &e in &els[1..] {
            let p = mid(self, e);
            mn = (mn.0.min(p.0), mn.1.min(p.1));
            mx = (mx.0.max(p.0), mx.1.max(p.1));
        }
        let (comp, pivot) = if mx.0 - mn.0 > mx.1 - mn.1 {
            (0, (mx.0 + mn.0) >> 1)
        } else {
            (1, (mx.1 + mn.1) >> 1)
        };
        let c = |s: &Self, e: usize| {
            if comp == 0 {
                s.elements[e].middle.0
            } else {
                s.elements[e].middle.1
            }
        };
        let n = els.len();
        let (mut lo, mut hi) = (0usize, n - 1);
        while lo < hi {
            while lo < hi && c(self, els[lo]) <= pivot {
                lo += 1;
            }
            while lo < hi && c(self, els[hi]) > pivot {
                hi -= 1;
            }
            if lo < hi {
                els.swap(lo, hi);
            }
        }
        if lo == n {
            lo = n >> 1;
        }
        let (l, r) = els.split_at_mut(lo);
        let left = self.build_tree(l);
        let right = self.build_tree(r);
        let (ln, rn) = (self.bvh[left], self.bvh[right]);
        self.bvh[node] = BvhNode {
            leaf: false,
            min: (ln.min.0.min(rn.min.0), ln.min.1.min(rn.min.1)),
            max: (ln.max.0.max(rn.max.0), ln.max.1.max(rn.max.1)),
            a: left,
            b: right,
        };
        node
    }

    fn intersect_nodes(&mut self, stack: &mut Vec<usize>, en: usize, tn: usize) -> bool {
        let (a, t) = (self.bvh[en], self.bvh[tn]);
        if a.min.0 >= t.max.0 || a.min.1 >= t.max.1 || a.max.0 <= t.min.0 || a.max.1 <= t.min.1 {
            return false;
        }
        let element = a.a;
        if t.leaf {
            let ne = t.a;
            if !self.elements[ne].processed || ne == element || self.equal_elements(ne, element) {
                return false;
            }
            let (el, nel) = (self.elements[element], self.elements[ne]);
            if el.degree == 1 && nel.degree == 1 {
                let ip = intersection_point(
                    self.pt(el.indices[0]),
                    self.pt(el.indices[1]),
                    self.pt(nel.indices[0]),
                    self.pt(nel.indices[1]),
                );
                if !ip.valid() {
                    return false;
                }
                self.points.push(ip.round());
                let pi = self.points.len() as u32 - 1;
                self.split_line_at(stack, tn, pi, !ip.accurate());
                self.split_line_at(stack, en, pi, false)
            } else {
                let mut axes: Vec<P> = vec![];
                self.append_separating_axes(&mut axes, element);
                self.append_separating_axes(&mut axes, ne);
                for &ax in &axes {
                    let r1 = self.separating_axis_range(ax, element);
                    let r2 = self.separating_axis_range(ax, ne);
                    if r1.0 >= r2.1 || r1.1 <= r2.0 {
                        return false;
                    }
                }
                if nel.degree > 1 {
                    self.split_curve(stack, tn);
                }
                if el.degree > 1 {
                    self.split_curve(stack, en);
                } else {
                    // treeNode may have become a split node above.
                    let t = self.bvh[tn];
                    if self.intersect_nodes(stack, en, t.a) {
                        return true;
                    }
                    let t = self.bvh[tn];
                    return self.intersect_nodes(stack, en, t.b);
                }
                true
            }
        } else {
            if self.intersect_nodes(stack, en, t.a) {
                return true;
            }
            let t = self.bvh[tn];
            self.intersect_nodes(stack, en, t.b)
        }
    }

    fn equal_elements(&self, a: usize, b: usize) -> bool {
        let (e1, e2) = (self.elements[a], self.elements[b]);
        if e1.degree != e2.degree {
            return false;
        }
        let d = e1.degree as usize;
        let same = (0..=d).all(|i| self.pt(e1.indices[i]) == self.pt(e2.indices[i]));
        let opp = (0..=d).all(|i| self.pt(e1.indices[d - i]) == self.pt(e2.indices[i]));
        same || opp
    }

    fn split_line_at(&mut self, stack: &mut Vec<usize>, node: usize, pi: u32, again: bool) -> bool {
        let first = self.bvh[node].a;
        let el = self.elements[first];
        let (u, v, p) = (self.pt(el.indices[0]), self.pt(el.indices[1]), self.pt(pi));
        if u == p || v == p {
            return false;
        }
        if again {
            self.elements[first].processed = false;
        }
        let second = self.new_element();
        self.elements[second] = self.elements[first];
        self.elements[first].indices[1] = pi;
        self.elements[second].indices[0] = pi;
        self.elements[first].middle = ((u.0 + p.0) >> 1, (u.1 + p.1) >> 1);
        self.elements[second].middle = ((v.0 + p.0) >> 1, (v.1 + p.1) >> 1);
        self.order.push(second);
        let nb = self.bvh[node];
        let mut l = BvhNode {
            leaf: true,
            min: nb.min,
            max: nb.max,
            a: first,
            b: 0,
        };
        let mut r = BvhNode {
            leaf: true,
            min: nb.min,
            max: nb.max,
            a: second,
            b: 0,
        };
        if u.0 < v.0 {
            l.max.0 = p.0;
            r.min.0 = p.0;
        } else {
            l.min.0 = p.0;
            r.max.0 = p.0;
        }
        if u.1 < v.1 {
            l.max.1 = p.1;
            r.min.1 = p.1;
        } else {
            l.min.1 = p.1;
            r.max.1 = p.1;
        }
        let (li, ri) = (self.new_bvh(l), self.new_bvh(r));
        self.elements[first].node = li;
        self.elements[second].node = ri;
        self.bvh[node] = BvhNode {
            leaf: false,
            min: nb.min,
            max: nb.max,
            a: li,
            b: ri,
        };
        if !self.elements[first].processed {
            stack.push(first);
            stack.push(second);
        }
        true
    }

    fn append_separating_axes(&self, axes: &mut Vec<P>, e: usize) {
        let el = self.elements[e];
        let n = |a: P, b: P| (a.1 - b.1, b.0 - a.0);
        let p = |i: usize| self.pt(el.indices[i]);
        let ns: Vec<P> = match el.degree {
            3 => {
                let (u, v, w, q) = (p(0), p(1), p(2), p(3));
                vec![n(u, v), n(v, w), n(w, q), n(q, u), n(u, w), n(v, q)]
            }
            2 => {
                let (u, v, w) = (p(0), p(1), p(2));
                vec![n(u, v), n(v, w), n(w, u)]
            }
            _ => vec![n(p(0), p(1))],
        };
        axes.extend(ns.into_iter().filter(|a| a.0 != 0 || a.1 != 0));
    }

    fn separating_axis_range(&self, axis: P, e: usize) -> (i32, i32) {
        let el = self.elements[e];
        let mut r = (0x7fffffff, -0x7fffffff);
        for i in 0..=el.degree as usize {
            let d = dot(axis, self.pt(el.indices[i]));
            r = (r.0.min(d), r.1.max(d));
        }
        r
    }

    fn split_curve(&mut self, stack: &mut Vec<usize>, node: usize) {
        let first = self.bvh[node].a;
        let second = self.new_element();
        self.elements[second] = self.elements[first];
        self.order.push(second);
        let fe = self.elements[first];
        let (u, v, w) = (
            self.pt(fe.indices[0]),
            self.pt(fe.indices[1]),
            self.pt(fe.indices[2]),
        );
        let mut accurate;
        if fe.degree == 2 {
            let mut pts = [(0, 0); 3];
            accurate = split_quadratic(u, v, w, &mut pts);
            let pi = self.points.len() as u32;
            self.points.push(pts[1]);
            accurate &= self.set_quadratic(first, fe.indices[0], pts[0], pi);
            let s2 = self.elements[second].indices[2];
            accurate &= self.set_quadratic(second, pi, pts[2], s2);
        } else {
            let q = self.pt(fe.indices[3]);
            let mut pts = [(0, 0); 5];
            accurate = split_cubic(u, v, w, q, &mut pts);
            let pi = self.points.len() as u32;
            self.points.push(pts[2]);
            accurate &= self.set_cubic(first, fe.indices[0], pts[0], pts[1], pi);
            let s3 = self.elements[second].indices[3];
            accurate &= self.set_cubic(second, pi, pts[3], pts[4], s3);
        }
        if !accurate {
            self.elements[first].processed = false;
            self.elements[second].processed = false;
        }
        let bb = |s: &Self, e: usize| {
            let el = s.elements[e];
            let (mut mn, mut mx) = ((i32::MAX, i32::MAX), (i32::MIN, i32::MIN));
            for i in 0..=el.degree as usize {
                let p = s.pt(el.indices[i]);
                mn = (mn.0.min(p.0), mn.1.min(p.1));
                mx = (mx.0.max(p.0), mx.1.max(p.1));
            }
            (mn, mx)
        };
        let (lb, rb) = (bb(self, first), bb(self, second));
        let li = self.new_bvh(BvhNode {
            leaf: true,
            min: lb.0,
            max: lb.1,
            a: first,
            b: 0,
        });
        let ri = self.new_bvh(BvhNode {
            leaf: true,
            min: rb.0,
            max: rb.1,
            a: second,
            b: 0,
        });
        self.elements[first].node = li;
        self.elements[second].node = ri;
        let nb = self.bvh[node];
        self.bvh[node] = BvhNode {
            leaf: false,
            min: nb.min,
            max: nb.max,
            a: li,
            b: ri,
        };
        if !self.elements[first].processed {
            stack.push(first);
            stack.push(second);
        }
    }

    fn set_quadratic(&mut self, e: usize, i1: u32, ctrl: P, i2: u32) -> bool {
        let (p1, p2) = (self.pt(i1), self.pt(i2));
        let np = self.points.len() as u32;
        let el = &mut self.elements[e];
        if flatten_quadratic(p1, ctrl, p2) {
            el.degree = 1;
            el.indices[0] = i1;
            el.indices[1] = i2;
            el.middle = ((p1.0 + p2.0) >> 1, (p1.1 + p2.1) >> 1);
            false
        } else {
            el.degree = 2;
            el.indices[0] = i1;
            el.indices[1] = np;
            el.indices[2] = i2;
            el.middle = ((p1.0 + ctrl.0 + p2.0) / 3, (p1.1 + ctrl.1 + p2.1) / 3);
            self.points.push(ctrl);
            true
        }
    }

    fn set_line(&mut self, e: usize, i1: u32, i2: u32) {
        let (u, q) = (self.pt(i1), self.pt(i2));
        let el = &mut self.elements[e];
        el.degree = 1;
        el.indices[0] = i1;
        el.indices[1] = i2;
        el.middle = ((u.0 + q.0) >> 1, (u.1 + q.1) >> 1);
    }

    fn set_cubic_raw(&mut self, e: usize, i1: u32, v: P, w: P, i2: u32) {
        let (u, q) = (self.pt(i1), self.pt(i2));
        let np = self.points.len() as u32;
        let el = &mut self.elements[e];
        el.degree = 3;
        el.indices = [i1, np, np + 1, i2];
        el.middle = ((u.0 + v.0 + w.0 + q.0) >> 2, (u.1 + v.1 + w.1 + q.1) >> 2);
        self.points.push(v);
        self.points.push(w);
    }

    fn set_cubic(&mut self, e: usize, i1: u32, v: P, w: P, i2: u32) -> bool {
        if flatten_cubic(self.pt(i1), v, w, self.pt(i2)) {
            self.set_line(e, i1, i2);
            false
        } else {
            self.set_cubic_raw(e, i1, v, w, i2);
            true
        }
    }

    fn set_cubic_and_simplify(&mut self, e: usize, i1: u32, v: P, w: P, i2: u32) {
        let (u, q) = (self.pt(i1), self.pt(i2));
        if flatten_cubic(u, v, w, q) {
            self.set_line(e, i1, i2);
            return;
        }
        if !(u == q || intersection_point(u, v, w, q).valid()) {
            self.set_cubic_raw(e, i1, v, w, i2);
            return;
        }
        let mut pts = [(0, 0); 5];
        split_cubic(u, v, w, q, &mut pts);
        let pi = self.points.len() as u32;
        self.points.push(pts[2]);
        let e2 = self.new_element();
        self.order.push(e2);
        self.set_cubic_and_simplify(e, i1, pts[0], pts[1], pi);
        self.set_cubic_and_simplify(e2, pi, pts[3], pts[4], i2);
    }

    fn find_element_left_of(
        &self,
        e: usize,
        bounds: (Option<usize>, Option<usize>),
    ) -> Option<usize> {
        if self.list.is_empty() {
            return None;
        }
        let mut cur = bounds.0.or(self.lfront());
        let mut result = None;
        while cur != bounds.1 && !self.element_is_left_of(e, cur.unwrap()) {
            result = cur;
            cur = self.lnext(cur.unwrap());
        }
        result
    }

    fn element_is_left_of(&self, l: usize, r: usize) -> bool {
        let (le, re) = (self.elements[l], self.elements[r]);
        let (lu, ll) = (self.pt(le.upper()), self.pt(le.lower()));
        let (ru, rl) = (self.pt(re.upper()), self.pt(re.lower()));
        if lu.0 < rl.0.min(ru.0) {
            return true;
        }
        if lu.0 > rl.0.max(ru.0) {
            return false;
        }
        let mut d = point_side_of_line(lu, rl, ru);
        if d == 0 {
            d = point_side_of_line(ll, rl, ru);
            if d == 0 {
                if re.degree > 1 {
                    d = point_side_of_line(ll, rl, self.pt(re.indices[1]));
                    if d == 0 {
                        d = point_side_of_line(ll, rl, self.pt(re.indices[2]));
                    }
                } else if le.degree > 1 {
                    d = point_side_of_line(self.pt(le.indices[1]), rl, ru);
                    if d == 0 {
                        d = point_side_of_line(self.pt(le.indices[2]), rl, ru);
                    }
                }
            }
        }
        d < 0
    }

    /// outerBounds: the nearest edges strictly left and right of the edges through `point`
    /// (Qt walks the red-black tree; on the ordered edge list this is the same pair).
    fn outer_bounds(&self, point: P) -> (Option<usize>, Option<usize>) {
        // -1: edge left of the point, 0: through it, 1: right of it.
        let class = |e: usize| -> i32 {
            let el = self.elements[e];
            let (v1, v2) = (self.pt(el.lower()), self.pt(el.upper()));
            if point == v1 || point == v2 {
                return 0;
            }
            let mut d = point_side_of_line(point, v1, v2);
            if d == 0 {
                if el.degree == 1 {
                    return 0;
                }
                d = point_side_of_line(point, v1, self.pt(el.indices[1]));
                if d == 0 {
                    d = point_side_of_line(point, v1, self.pt(el.indices[2]));
                }
            }
            if d < 0 { 1 } else { -1 }
        };
        let mut first = None;
        let mut second = None;
        for &e in &self.list {
            match class(e) {
                -1 => first = Some(e),
                1 => {
                    second = Some(e);
                    break;
                }
                _ => {}
            }
        }
        (first, second)
    }

    fn sub_div_quadratic(&mut self, u: P, v: P, w: P) {
        if flatten_quadratic(u, v, w) {
            return;
        }
        let mut pts = [(0, 0); 3];
        split_quadratic(u, v, w, &mut pts);
        self.sub_div_quadratic(u, pts[0], pts[1]);
        self.indices.push(self.points.len() as u32);
        self.points.push(pts[1]);
        self.sub_div_quadratic(pts[1], pts[2], w);
    }

    fn sub_div_cubic(&mut self, u: P, v: P, w: P, q: P) {
        if flatten_cubic(u, v, w, q) {
            return;
        }
        let mut pts = [(0, 0); 5];
        split_cubic(u, v, w, q, &mut pts);
        self.sub_div_cubic(u, pts[0], pts[1], pts[2]);
        self.indices.push(self.points.len() as u32);
        self.points.push(pts[2]);
        self.sub_div_cubic(pts[2], pts[3], pts[4], q);
    }
}

const FS: i32 = 256;

fn flatten_quadratic(u: P, v: P, w: P) -> bool {
    let (d0, d1) = (sub(v, u), sub(w, v));
    let d = cross(d0, d1).wrapping_abs();
    let l = d0.0.abs() + d0.1.abs() + d1.0.abs() + d1.1.abs();
    d < FS * FS * 3 / 2 || l <= FS * 2
}

fn flatten_cubic(u: P, v: P, w: P, q: P) -> bool {
    let ds = [sub(v, u), sub(w, v), sub(q, w), sub(q, u)];
    let d = cross(ds[0], ds[1])
        .wrapping_abs()
        .wrapping_add(cross(ds[1], ds[2]).wrapping_abs())
        .wrapping_add(cross(ds[0], ds[3]).wrapping_abs())
        .wrapping_add(cross(ds[3], ds[2]).wrapping_abs());
    let l = ds[0].0.abs()
        + ds[0].1.abs()
        + ds[1].0.abs()
        + ds[1].1.abs()
        + ds[2].0.abs()
        + ds[2].1.abs();
    d < FS * FS * 3 || l <= FS * 2
}

fn add(a: P, b: P) -> P {
    (a.0 + b.0, a.1 + b.1)
}

fn split_quadratic(u: P, v: P, w: P, r: &mut [P; 3]) -> bool {
    r[0] = add(u, v);
    r[2] = add(v, w);
    r[1] = add(r[0], r[2]);
    let acc = ((r[0].0 | r[0].1 | r[2].0 | r[2].1) & 1) == 0 && ((r[1].0 | r[1].1) & 3) == 0;
    r[0] = (r[0].0 >> 1, r[0].1 >> 1);
    r[1] = (r[1].0 >> 2, r[1].1 >> 2);
    r[2] = (r[2].0 >> 1, r[2].1 >> 1);
    acc
}

fn split_cubic(u: P, v: P, w: P, q: P, r: &mut [P; 5]) -> bool {
    r[0] = add(u, v);
    r[2] = add(v, w);
    r[4] = add(w, q);
    r[1] = add(r[0], r[2]);
    r[3] = add(r[2], r[4]);
    r[2] = add(r[1], r[3]);
    let acc = ((r[0].0 | r[0].1 | r[4].0 | r[4].1) & 1) == 0
        && ((r[1].0 | r[1].1 | r[3].0 | r[3].1) & 3) == 0
        && ((r[2].0 | r[2].1) & 7) == 0;
    r[0] = (r[0].0 >> 1, r[0].1 >> 1);
    r[1] = (r[1].0 >> 2, r[1].1 >> 2);
    r[2] = (r[2].0 >> 3, r[2].1 >> 3);
    r[3] = (r[3].0 >> 2, r[3].1 >> 2);
    r[4] = (r[4].0 >> 1, r[4].1 >> 1);
    acc
}

/// sortEvents: bucket sort on y, then insertion sort with Event::operator<.
fn sort_events(events: &mut [Event]) {
    let count = events.len();
    let (mut mn, mut mx) = (events[0].point.1, events[0].point.1);
    for e in &events[1..] {
        mn = mn.min(e.point.1);
        mx = mx.max(e.point.1);
    }
    let mut counts = [0usize; 0x101];
    let bins: Vec<usize> = events
        .iter()
        .map(|e| ((((mx - e.point.1) as i64) << 8) / ((mx - mn) as i64 + 1)) as usize)
        .collect();
    for &b in &bins {
        counts[b] += 1;
    }
    for i in 1..0x100 {
        counts[i] += counts[i - 1];
    }
    counts[0x100] = counts[0xff];
    let mut buffer = events.to_vec();
    for i in 0..count {
        counts[bins[i]] -= 1;
        buffer[counts[bins[i]]] = events[i];
    }
    let mut j = 0;
    for i in 0..0x100 {
        while j < counts[i + 1] {
            let mut k = j;
            while k > 0 && ev_lt(&buffer[j], &events[k - 1]) {
                events[k] = events[k - 1];
                k -= 1;
            }
            events[k] = buffer[j];
            j += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type F = (f64, f64);

    /// Output polygons in path units (24.8 fixed / 256), split at END.
    fn polys(out: &(Vec<P>, Vec<u32>)) -> Vec<Vec<F>> {
        let mut r = vec![];
        let mut cur = vec![];
        for &i in &out.1 {
            if i == END {
                r.push(std::mem::take(&mut cur));
            } else {
                let p = out.0[i as usize];
                cur.push((p.0 as f64 / SCALE, p.1 as f64 / SCALE));
            }
        }
        assert!(cur.is_empty(), "index list must end with END");
        r
    }

    fn signed_area(p: &[F]) -> f64 {
        let n = p.len();
        (0..n)
            .map(|i| p[i].0 * p[(i + 1) % n].1 - p[(i + 1) % n].0 * p[i].1)
            .sum::<f64>()
            * 0.5
    }

    fn total_area(ps: &[Vec<F>]) -> f64 {
        ps.iter().map(|p| signed_area(p)).sum::<f64>().abs()
    }

    fn winding(ps: &[Vec<F>], pt: F) -> i32 {
        let mut w = 0;
        for p in ps {
            for i in 0..p.len() {
                let (a, b) = (p[i], p[(i + 1) % p.len()]);
                let side = (b.0 - a.0) * (pt.1 - a.1) - (pt.0 - a.0) * (b.1 - a.1);
                if a.1 <= pt.1 && b.1 > pt.1 && side > 0.0 {
                    w += 1;
                } else if a.1 > pt.1 && b.1 <= pt.1 && side < 0.0 {
                    w -= 1;
                }
            }
        }
        w
    }

    /// No two output edges cross properly (qSimplifyPath output is non-self-intersecting).
    fn assert_simple(ps: &[Vec<F>]) {
        let edges: Vec<(F, F)> = ps
            .iter()
            .flat_map(|p| (0..p.len()).map(move |i| (p[i], p[(i + 1) % p.len()])))
            .collect();
        let orient = |a: F, b: F, c: F| {
            let v = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
            if v.abs() < 1e-9 { 0 } else { v.signum() as i32 }
        };
        for i in 0..edges.len() {
            for j in i + 1..edges.len() {
                let ((a, b), (c, d)) = (edges[i], edges[j]);
                let (o1, o2, o3, o4) = (
                    orient(a, b, c),
                    orient(a, b, d),
                    orient(c, d, a),
                    orient(c, d, b),
                );
                assert!(
                    !(o1 * o2 < 0 && o3 * o4 < 0),
                    "edges {:?} and {:?} cross",
                    edges[i],
                    edges[j]
                );
            }
        }
    }

    fn poly_path(pts: &[F], close: bool) -> Vec<PathEl> {
        let mut v = vec![PathEl::Move(pts[0].0, pts[0].1)];
        v.extend(pts[1..].iter().map(|p| PathEl::Line(p.0, p.1)));
        if close {
            v.push(PathEl::Line(pts[0].0, pts[0].1));
        }
        v
    }

    fn square(x: f64, y: f64, s: f64, ccw: bool) -> Vec<PathEl> {
        let mut pts = vec![(x, y), (x + s, y), (x + s, y + s), (x, y + s)];
        if ccw {
            pts.reverse();
        }
        poly_path(&pts, true)
    }

    /// QPainterPath::cubicTo segments approximating a circle (kappa = 0.5523).
    fn circle(cx: f64, cy: f64, r: f64) -> Vec<PathEl> {
        let k = r * 0.552_284_749_830_793_4;
        vec![
            PathEl::Move(cx + r, cy),
            PathEl::Cubic(cx + r, cy + k, cx + k, cy + r, cx, cy + r),
            PathEl::Cubic(cx - k, cy + r, cx - r, cy + k, cx - r, cy),
            PathEl::Cubic(cx - r, cy - k, cx - k, cy - r, cx, cy - r),
            PathEl::Cubic(cx + k, cy - r, cx + r, cy - k, cx + r, cy),
        ]
    }

    #[test]
    fn empty_path_gives_nothing() {
        assert_eq!(simplify(&[], 1.0, 0.0), (vec![], vec![]));
    }

    // Vertices are the path points mapped by translate(t) * scale(s), in Q_FIXED_POINT_SCALE
    // (256) units rounded with qRound.
    #[test]
    fn square_maps_to_fixed_point_polygon() {
        let out = simplify(&square(1.0, 2.0, 10.0, false), 2.0, 3.0);
        let ps = polys(&out);
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].len(), 4);
        let mut got: Vec<P> = out
            .1
            .iter()
            .filter(|&&i| i != END)
            .map(|&i| out.0[i as usize])
            .collect();
        got.sort();
        let fx = |v: f64| (v * 2.0 + 3.0) as i32 * 256;
        let mut want = vec![
            (fx(1.0), fx(2.0)),
            (fx(11.0), fx(2.0)),
            (fx(11.0), fx(12.0)),
            (fx(1.0), fx(12.0)),
        ];
        want.sort();
        assert_eq!(got, want);
        assert!((total_area(&ps) - 400.0).abs() < 1e-9);
    }

    // qRound(x * 256): half a fixed unit rounds away from zero.
    #[test]
    fn fixed_point_rounding() {
        let h = 0.5 / 256.0;
        let out = simplify(
            &poly_path(&[(h, h), (10.0, 0.0), (10.0, 10.0), (-h, 10.0)], true),
            1.0,
            0.0,
        );
        assert!(out.0.contains(&(1, 1)));
        assert!(out.0.contains(&(-1, 2560)));
    }

    // Subpaths are implicitly closed (qSimplifyPath adds the closing edge for every subpath).
    #[test]
    fn open_subpath_is_closed() {
        let pts = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let open = polys(&simplify(&poly_path(&pts, false), 1.0, 0.0));
        let closed = polys(&simplify(&poly_path(&pts, true), 1.0, 0.0));
        assert_eq!(open.len(), 1);
        assert!((total_area(&open) - 100.0).abs() < 1e-9);
        assert!((total_area(&closed) - 100.0).abs() < 1e-9);
    }

    // connectElements keeps edges between winding 0 and +-1 regions: overlapping
    // same-direction squares give their union (non-zero / WindingFill), with the
    // intersection points (10,5) and (5,10) as new vertices.
    #[test]
    fn overlapping_squares_union() {
        let mut path = square(0.0, 0.0, 10.0, false);
        path.extend(square(5.0, 5.0, 10.0, false));
        let out = simplify(&path, 1.0, 0.0);
        let ps = polys(&out);
        assert_simple(&ps);
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].len(), 8);
        assert!((total_area(&ps) - 175.0).abs() < 1e-9);
        assert!(ps[0].contains(&(10.0, 5.0)) && ps[0].contains(&(5.0, 10.0)));
        assert_ne!(winding(&ps, (7.5, 7.5)), 0);
    }

    // Opposite directions: the overlap has winding 0 and drops out.
    #[test]
    fn opposite_squares_cancel_overlap() {
        let mut path = square(0.0, 0.0, 10.0, false);
        path.extend(square(5.0, 5.0, 10.0, true));
        let ps = polys(&simplify(&path, 1.0, 0.0));
        assert_simple(&ps);
        assert!((total_area(&ps) - 150.0).abs() < 1e-9);
        assert_eq!(winding(&ps, (7.5, 7.5)), 0);
        assert_ne!(winding(&ps, (2.0, 2.0)), 0);
        assert_ne!(winding(&ps, (12.0, 12.0)), 0);
    }

    // A same-direction inner square (winding 2) is not a hole under the winding rule; a
    // reversed one is, and comes out as a separate, oppositely oriented polygon.
    #[test]
    fn nested_squares() {
        let mut same = square(0.0, 0.0, 10.0, false);
        same.extend(square(2.0, 2.0, 6.0, false));
        let ps = polys(&simplify(&same, 1.0, 0.0));
        assert_eq!(ps.len(), 1);
        assert!((total_area(&ps) - 100.0).abs() < 1e-9);

        let mut hole = square(0.0, 0.0, 10.0, false);
        hole.extend(square(2.0, 2.0, 6.0, true));
        let ps = polys(&simplify(&hole, 1.0, 0.0));
        assert_simple(&ps);
        assert_eq!(ps.len(), 2);
        assert!(signed_area(&ps[0]) * signed_area(&ps[1]) < 0.0);
        assert!((total_area(&ps) - 64.0).abs() < 1e-9);
        assert_eq!(winding(&ps, (5.0, 5.0)), 0);
        assert_ne!(winding(&ps, (1.0, 5.0)), 0);
    }

    // removeIntersections splits both edges of a bow tie at the crossing (5,5); the output
    // (two triangles, or one polygon touching itself there) visits (5,5) twice and keeps both
    // lobes with the same orientation-independent area of 25 each.
    #[test]
    fn bow_tie_is_split_at_crossing() {
        let path = poly_path(&[(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)], true);
        let ps = polys(&simplify(&path, 1.0, 0.0));
        assert_simple(&ps);
        let visits = ps.iter().flatten().filter(|&&v| v == (5.0, 5.0)).count();
        assert_eq!(visits, 2);
        assert_eq!(ps.iter().map(Vec::len).sum::<usize>(), 6);
        assert!((ps.iter().map(|p| signed_area(p).abs()).sum::<f64>() - 50.0).abs() < 1e-9);
        assert_eq!(winding(&ps, (5.0, 2.0)), 0);
        assert_eq!(winding(&ps, (5.0, 8.0)), 0);
        assert_ne!(winding(&ps, (2.0, 5.0)), 0);
        assert_ne!(winding(&ps, (8.0, 5.0)), 0);
    }

    // Pentagram: the centre has winding 2, so the winding rule fills it (an odd-even fill
    // would leave a pentagon hole).
    #[test]
    fn pentagram_centre_is_filled() {
        let pt = |k: usize| {
            let a = -std::f64::consts::FRAC_PI_2 + k as f64 * 4.0 * std::f64::consts::PI / 5.0;
            (20.0 + 10.0 * a.cos(), 20.0 + 10.0 * a.sin())
        };
        let path = poly_path(&(0..5).map(pt).collect::<Vec<_>>(), true);
        let ps = polys(&simplify(&path, 1.0, 0.0));
        assert_simple(&ps);
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].len(), 10);
        assert_ne!(winding(&ps, (20.0, 20.0)), 0);
    }

    // flattenCubic: a cubic becomes a chord once its control-polygon cross products sum to
    // < 3 px^2 (Q_FIXED_POINT_SCALE^2 * 3), so the result is an inscribed polygon whose
    // accuracy is fixed in device pixels: scaling the path up by 8 makes it relatively more
    // accurate.
    #[test]
    fn circle_is_flattened() {
        let exact = std::f64::consts::PI * 100.0;
        let ps = polys(&simplify(&circle(20.0, 20.0, 10.0), 1.0, 0.0));
        assert_simple(&ps);
        assert_eq!(ps.len(), 1);
        assert!(ps[0].len() >= 8);
        for &(x, y) in &ps[0] {
            let d = (x - 20.0).hypot(y - 20.0);
            assert!((9.99..=10.01).contains(&d), "{d}");
        }
        let a = total_area(&ps);
        assert!(a < exact && exact - a < 12.0, "{a}");

        let ps = polys(&simplify(&circle(20.0, 20.0, 10.0), 8.0, 0.0));
        assert_simple(&ps);
        let a8 = total_area(&ps) / 64.0;
        assert!(a8 < exact && exact - a8 < (exact - a) / 4.0, "{a8}");
    }

    // QPainterPath::quadTo stores a cubic with c1 = p0 + 2/3 (q - p0), c2 = p + 2/3 (q - p);
    // the simplifier recognises it as a quadratic (flattenQuadratic: chord once the cross
    // product is < 1.5 px^2). Parabola segment + chord: 2/3 * base * height, approached
    // from below by the inscribed polygon.
    #[test]
    fn quadratic_segment_area() {
        let (p0, q, p) = ((0.0, 0.0), (10.0, 20.0), (20.0, 0.0));
        let c1 = (
            p0.0 + 2.0 / 3.0 * (q.0 - p0.0),
            p0.1 + 2.0 / 3.0 * (q.1 - p0.1),
        );
        let c2 = (p.0 + 2.0 / 3.0 * (q.0 - p.0), p.1 + 2.0 / 3.0 * (q.1 - p.1));
        let path = vec![
            PathEl::Move(p0.0, p0.1),
            PathEl::Cubic(c1.0, c1.1, c2.0, c2.1, p.0, p.1),
        ];
        let ps = polys(&simplify(&path, 1.0, 0.0));
        assert_simple(&ps);
        assert_eq!(ps.len(), 1);
        for &(x, y) in &ps[0] {
            assert!(y <= 10.0 + 1e-9 && (0.0..=20.0).contains(&x));
        }
        let exact = 2.0 / 3.0 * 20.0 * 10.0;
        let a = total_area(&ps);
        assert!(a < exact && exact - a < 4.0, "{a}");
    }

    // A path that only retraces itself encloses nothing.
    #[test]
    fn zero_area_path_has_no_polygons() {
        let path = poly_path(&[(0.0, 0.0), (10.0, 0.0)], false);
        let ps = polys(&simplify(&path, 1.0, 0.0));
        assert!(total_area(&ps) < 1e-9);
        assert!(
            ps.iter()
                .all(|p| p.is_empty() || signed_area(p).abs() < 1e-9)
        );
    }
}
