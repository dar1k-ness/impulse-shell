//! qTriangulate(const QPainterPath &) of Qt 6.12's qtriangulator.cpp, for a path of lines:
//! the triangles `QSGCurveProcessor::processFill` fills the inside of a shape with.
#![allow(clippy::too_many_lines)]

use std::collections::HashSet;

const QTRIANGULATOR_MAX: i32 = 1 << 21;
const Q_FIXED_POINT_SCALE: f64 = 32.0;
const FIXED_POINT_MAX: f64 = (QTRIANGULATOR_MAX - 1) as f64 / Q_FIXED_POINT_SCALE;

/// qRound(double).
fn q_round(d: f64) -> i32 {
    if d >= 0.0 {
        (d + 0.5) as i32
    } else {
        (d - 0.5) as i32
    }
}

fn to_clamped_fixed_point(c: f64) -> i32 {
    q_round(c.clamp(-FIXED_POINT_MAX, FIXED_POINT_MAX) * Q_FIXED_POINT_SCALE)
}

// ---------------------------------------------------------------- QFraction

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Fraction {
    numerator: u64,
    denominator: u64,
}

impl Fraction {
    fn is_valid(&self) -> bool {
        self.denominator != 0
    }
    fn lt(&self, o: &Fraction) -> bool {
        compare_fractions(self.numerator, self.denominator, o.numerator, o.denominator) < 0
    }
}

fn gcd(mut x: u64, mut y: u64) -> u64 {
    while y != 0 {
        let z = y;
        y = x % y;
        x = z;
    }
    x
}

fn cmp_u64(a: u64, b: u64) -> i32 {
    (a > b) as i32 - (a < b) as i32
}

fn compare_fractions(mut a: u64, mut b: u64, mut c: u64, mut d: u64) -> i32 {
    const LIMIT: u64 = 0x1_0000_0000;
    loop {
        if b < LIMIT && d < LIMIT {
            return cmp_u64(a.wrapping_mul(d), b.wrapping_mul(c));
        }
        if a == 0 || c == 0 {
            return cmp_u64(a, c);
        }
        let b_div_a = b / a;
        let d_div_c = d / c;
        if b_div_a != d_div_c {
            return cmp_u64(d_div_c, b_div_a);
        }
        d -= d_div_c * c;
        b -= b_div_a * a;
        std::mem::swap(&mut a, &mut d);
        std::mem::swap(&mut b, &mut c);
    }
}

fn fraction(n: u64, d: u64) -> Fraction {
    if n == 0 {
        Fraction {
            numerator: 0,
            denominator: 1,
        }
    } else {
        let g = gcd(n, d);
        Fraction {
            numerator: n / g,
            denominator: d / g,
        }
    }
}

// ---------------------------------------------------------------- QPodPoint

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Pt {
    x: i32,
    y: i32,
}

impl Pt {
    fn lt(&self, o: &Pt) -> bool {
        if self.y != o.y {
            self.y < o.y
        } else {
            self.x < o.x
        }
    }
    fn sub(self, o: Pt) -> Pt {
        Pt {
            x: self.x.wrapping_sub(o.x),
            y: self.y.wrapping_sub(o.y),
        }
    }
}

fn cross(u: Pt, v: Pt) -> i64 {
    u.x as i64 * v.y as i64 - u.y as i64 * v.x as i64
}

/// Positive if `p` is to the right of the line `v1` -> `v2`, negative if left, zero if on it.
fn point_distance_from_line(p: Pt, v1: Pt, v2: Pt) -> i64 {
    cross(v2.sub(v1), p.sub(v1))
}

fn point_is_left_of_line(p: Pt, v1: Pt, v2: Pt) -> bool {
    point_distance_from_line(p, v1, v2) < 0
}

// ---------------------------------------------------------------- QIntersectionPoint

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct IPoint {
    upper_left: Pt,
    x_offset: Fraction,
    y_offset: Fraction,
}

impl IPoint {
    fn is_valid(&self) -> bool {
        self.x_offset.is_valid() && self.y_offset.is_valid()
    }
    fn round(&self) -> Pt {
        let mut r = self.upper_left;
        if 2 * self.x_offset.numerator >= self.x_offset.denominator {
            r.x += 1;
        }
        if 2 * self.y_offset.numerator >= self.y_offset.denominator {
            r.y += 1;
        }
        r
    }
    fn is_accurate(&self) -> bool {
        self.x_offset.numerator == 0 && self.y_offset.numerator == 0
    }
    fn lt(&self, o: &IPoint) -> bool {
        if self.upper_left.y != o.upper_left.y {
            return self.upper_left.y < o.upper_left.y;
        }
        if self.y_offset != o.y_offset {
            return self.y_offset.lt(&o.y_offset);
        }
        if self.upper_left.x != o.upper_left.x {
            return self.upper_left.x < o.upper_left.x;
        }
        self.x_offset.lt(&o.x_offset)
    }
    /// Whether this point is on the infinite line through `u` and `v`.
    fn is_on_line(&self, u: Pt, v: Pt) -> bool {
        let p = self.upper_left.sub(u);
        let q = v.sub(u);
        let is_horizontal = p.y == 0 && self.y_offset.numerator == 0;
        let is_vertical = p.x == 0 && self.x_offset.numerator == 0;
        if is_horizontal && is_vertical {
            return true;
        }
        if is_horizontal {
            return q.y == 0;
        }
        if q.y == 0 {
            return false;
        }
        if is_vertical {
            return q.x == 0;
        }
        if q.x == 0 {
            return false;
        }
        if ((q.x < 0) == (q.y < 0)) != ((p.x < 0) == (p.y < 0)) {
            return false;
        }
        let nx = if p.x < 0 {
            ((-p.x) as u64)
                .wrapping_mul(self.x_offset.denominator)
                .wrapping_sub(self.x_offset.numerator)
        } else {
            (p.x as u64)
                .wrapping_mul(self.x_offset.denominator)
                .wrapping_add(self.x_offset.numerator)
        };
        let ny = if p.y < 0 {
            ((-p.y) as u64)
                .wrapping_mul(self.y_offset.denominator)
                .wrapping_sub(self.y_offset.numerator)
        } else {
            (p.y as u64)
                .wrapping_mul(self.y_offset.denominator)
                .wrapping_add(self.y_offset.numerator)
        };
        fraction(
            (q.x.unsigned_abs() as u64).wrapping_mul(self.x_offset.denominator),
            (q.y.unsigned_abs() as u64).wrapping_mul(self.y_offset.denominator),
        ) == fraction(nx, ny)
    }
}

fn ipoint_of(p: Pt) -> IPoint {
    IPoint {
        upper_left: p,
        x_offset: Fraction {
            numerator: 0,
            denominator: 1,
        },
        y_offset: Fraction {
            numerator: 0,
            denominator: 1,
        },
    }
}

/// Where the interiors of `u1`-`u2` and `v1`-`v2` cross; invalid if they do not.
fn ipoint_of_lines(u1: Pt, u2: Pt, v1: Pt, v2: Pt) -> IPoint {
    let mut result = IPoint::default();
    let u = u2.sub(u1);
    let v = v2.sub(v1);
    let mut d1 = cross(u, v1.sub(u1));
    let mut d2 = cross(u, v2.sub(u1));
    let mut det = d2 - d1;
    let mut d3 = cross(v, u1.sub(v1));
    let mut d4 = d3 - det;
    if det == 0 {
        return result;
    }
    if det < 0 {
        det = -det;
        d1 = -d1;
        d2 = -d2;
        d3 = -d3;
        d4 = -d4;
    }
    if d1 >= 0 || d2 <= 0 || d3 <= 0 || d4 >= 0 {
        return result;
    }
    let (vx, vy) = (v.x as i64, v.y as i64);
    if v.x >= 0 {
        result.upper_left.x = v1.x + ((-vx * d1) / det) as i32;
        result.x_offset = fraction(((-vx * d1) as u64) % (det as u64), det as u64);
    } else {
        result.upper_left.x = v2.x + ((-vx * d2) / det) as i32;
        result.x_offset = fraction(((-vx * d2) as u64) % (det as u64), det as u64);
    }
    if v.y >= 0 {
        result.upper_left.y = v1.y + ((-vy * d1) / det) as i32;
        result.y_offset = fraction(((-vy * d1) as u64) % (det as u64), det as u64);
    } else {
        result.upper_left.y = v2.y + ((-vy * d2) / det) as i32;
        result.y_offset = fraction(((-vy * d2) as u64) % (det as u64), det as u64);
    }
    result
}

// ---------------------------------------------------------------- QMaxHeap

struct MaxHeap<T> {
    data: Vec<T>,
}

impl<T: Copy> MaxHeap<T> {
    fn new() -> Self {
        MaxHeap { data: Vec::new() }
    }
    fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
    fn top(&self) -> &T {
        &self.data[0]
    }
    fn push(&mut self, x: T, lt: impl Fn(&T, &T) -> bool) {
        let mut current = self.data.len();
        self.data.push(x);
        while current != 0 {
            let parent = (current - 1) / 2;
            if !lt(&self.data[parent], &x) {
                break;
            }
            self.data[current] = self.data[parent];
            current = parent;
        }
        self.data[current] = x;
    }
    fn pop(&mut self, lt: impl Fn(&T, &T) -> bool) -> T {
        let result = self.data[0];
        let back = self.data.pop().unwrap();
        if !self.data.is_empty() {
            let mut current = 0;
            loop {
                let left = 2 * current + 1;
                let right = 2 * current + 2;
                if left >= self.data.len() {
                    break;
                }
                let mut greater = left;
                if right < self.data.len() && lt(&self.data[left], &self.data[right]) {
                    greater = right;
                }
                if lt(&self.data[greater], &back) {
                    break;
                }
                self.data[current] = self.data[greater];
                current = greater;
            }
            self.data[current] = back;
        }
        result
    }
}

// ---------------------------------------------------------------- QRBTree

const NIL: usize = usize::MAX;

#[derive(Clone, Copy)]
struct RbNode {
    data: i32,
    parent: usize,
    left: usize,
    right: usize,
    red: bool,
}

/// QRBTree<int>: an ordered sequence kept as a red-black tree; nodes are indices into `n`.
struct RbTree {
    n: Vec<RbNode>,
    root: usize,
    free: usize,
}

impl RbTree {
    fn new() -> Self {
        RbTree {
            n: Vec::new(),
            root: NIL,
            free: NIL,
        }
    }

    fn set_ref(&mut self, node: usize, to: usize) {
        let p = self.n[node].parent;
        if p == NIL {
            self.root = to;
        } else if node == self.n[p].left {
            self.n[p].left = to;
        } else {
            self.n[p].right = to;
        }
    }

    fn rotate_left(&mut self, node: usize) {
        let r = self.n[node].right;
        self.set_ref(node, r);
        self.n[r].parent = self.n[node].parent;
        let rl = self.n[r].left;
        self.n[node].right = rl;
        if rl != NIL {
            self.n[rl].parent = node;
        }
        self.n[r].left = node;
        self.n[node].parent = r;
    }

    fn rotate_right(&mut self, node: usize) {
        let l = self.n[node].left;
        self.set_ref(node, l);
        self.n[l].parent = self.n[node].parent;
        let lr = self.n[l].right;
        self.n[node].left = lr;
        if lr != NIL {
            self.n[lr].parent = node;
        }
        self.n[l].right = node;
        self.n[node].parent = l;
    }

    fn is_red(&self, node: usize) -> bool {
        node != NIL && self.n[node].red
    }

    fn update(&mut self, mut node: usize) {
        loop {
            let mut parent = self.n[node].parent;
            if parent == NIL {
                self.n[node].red = false;
                return;
            }
            if !self.n[parent].red {
                return;
            }
            let grandpa = self.n[parent].parent;
            let uncle = if parent == self.n[grandpa].left {
                self.n[grandpa].right
            } else {
                self.n[grandpa].left
            };
            if self.is_red(uncle) {
                self.n[parent].red = false;
                self.n[uncle].red = false;
                self.n[grandpa].red = true;
                node = grandpa;
                continue;
            }
            if node == self.n[parent].right && parent == self.n[grandpa].left {
                node = parent;
                self.rotate_left(node);
            } else if node == self.n[parent].left && parent == self.n[grandpa].right {
                node = parent;
                self.rotate_right(node);
            }
            parent = self.n[node].parent;
            if parent == self.n[grandpa].left {
                self.rotate_right(grandpa);
            } else {
                self.rotate_left(grandpa);
            }
            self.n[parent].red = false;
            self.n[grandpa].red = true;
            return;
        }
    }

    fn attach_left(&mut self, parent: usize, child: usize) {
        self.n[parent].left = child;
        self.n[child].parent = parent;
        self.update(child);
    }

    fn attach_right(&mut self, parent: usize, child: usize) {
        self.n[parent].right = child;
        self.n[child].parent = parent;
        self.update(child);
    }

    fn attach_after(&mut self, parent: usize, child: usize) {
        if self.root == NIL {
            self.root = child;
            self.update(child);
        } else if parent == NIL {
            let f = self.front(self.root);
            self.attach_left(f, child);
        } else if self.n[parent].right != NIL {
            let f = self.front(self.n[parent].right);
            self.attach_left(f, child);
        } else {
            self.attach_right(parent, child);
        }
    }

    fn swap_nodes(&mut self, n1: usize, n2: usize) {
        if self.n[n1].parent == n2 {
            self.n[n1].parent = self.n[n2].parent;
            self.n[n2].parent = n1;
        } else if self.n[n2].parent == n1 {
            self.n[n2].parent = self.n[n1].parent;
            self.n[n1].parent = n2;
        } else {
            let (p1, p2) = (self.n[n1].parent, self.n[n2].parent);
            self.n[n1].parent = p2;
            self.n[n2].parent = p1;
        }
        let (a, b) = (self.n[n1], self.n[n2]);
        self.n[n1].left = b.left;
        self.n[n2].left = a.left;
        self.n[n1].right = b.right;
        self.n[n2].right = a.right;
        self.n[n1].red = b.red;
        self.n[n2].red = a.red;

        let p1 = self.n[n1].parent;
        if p1 != NIL {
            if self.n[p1].left == n2 {
                self.n[p1].left = n1;
            } else {
                self.n[p1].right = n1;
            }
        } else {
            self.root = n1;
        }
        let p2 = self.n[n2].parent;
        if p2 != NIL {
            if self.n[p2].left == n1 {
                self.n[p2].left = n2;
            } else {
                self.n[p2].right = n2;
            }
        } else {
            self.root = n2;
        }
        for n in [n1, n2] {
            let (l, r) = (self.n[n].left, self.n[n].right);
            if l != NIL {
                self.n[l].parent = n;
            }
            if r != NIL {
                self.n[r].parent = n;
            }
        }
    }

    fn detach(&mut self, node: usize) {
        if self.n[node].right != NIL {
            let f = self.front(self.n[node].right);
            self.swap_nodes(node, f);
        }
        let child = if self.n[node].left != NIL {
            self.n[node].left
        } else {
            self.n[node].right
        };
        if !self.n[node].red {
            if self.is_red(child) {
                self.n[child].red = false;
            } else {
                self.rebalance(node);
            }
        }
        self.set_ref(node, child);
        if child != NIL {
            self.n[child].parent = self.n[node].parent;
        }
        self.n[node].left = NIL;
        self.n[node].right = NIL;
        self.n[node].parent = NIL;
    }

    fn sibling(&self, node: usize) -> usize {
        let p = self.n[node].parent;
        if node == self.n[p].left {
            self.n[p].right
        } else {
            self.n[p].left
        }
    }

    fn rebalance(&mut self, mut node: usize) {
        loop {
            let parent = self.n[node].parent;
            if parent == NIL {
                return;
            }
            let mut sibling = self.sibling(node);
            if self.n[sibling].red {
                self.n[sibling].red = false;
                self.n[parent].red = true;
                if node == self.n[parent].left {
                    self.rotate_left(parent);
                } else {
                    self.rotate_right(parent);
                }
                sibling = self.sibling(node);
            }
            let parent = self.n[node].parent;
            if !self.is_red(self.n[sibling].left) && !self.is_red(self.n[sibling].right) {
                let parent_was_red = self.n[parent].red;
                self.n[sibling].red = true;
                self.n[parent].red = false;
                if parent_was_red {
                    return;
                }
                node = parent;
                continue;
            }
            if node == self.n[parent].left {
                if !self.is_red(self.n[sibling].right) {
                    self.n[sibling].red = true;
                    let sl = self.n[sibling].left;
                    self.n[sl].red = false;
                    self.rotate_right(sibling);
                    sibling = self.n[sibling].parent;
                }
                self.n[sibling].red = self.n[parent].red;
                self.n[parent].red = false;
                let sr = self.n[sibling].right;
                self.n[sr].red = false;
                self.rotate_left(parent);
            } else {
                if !self.is_red(self.n[sibling].left) {
                    self.n[sibling].red = true;
                    let sr = self.n[sibling].right;
                    self.n[sr].red = false;
                    self.rotate_left(sibling);
                    sibling = self.n[sibling].parent;
                }
                self.n[sibling].red = self.n[parent].red;
                self.n[parent].red = false;
                let sl = self.n[sibling].left;
                self.n[sl].red = false;
                self.rotate_right(parent);
            }
            return;
        }
    }

    fn front(&self, mut node: usize) -> usize {
        while self.n[node].left != NIL {
            node = self.n[node].left;
        }
        node
    }

    fn back(&self, mut node: usize) -> usize {
        while self.n[node].right != NIL {
            node = self.n[node].right;
        }
        node
    }

    fn next(&self, mut node: usize) -> usize {
        if self.n[node].right != NIL {
            return self.front(self.n[node].right);
        }
        while self.n[node].parent != NIL && node == self.n[self.n[node].parent].right {
            node = self.n[node].parent;
        }
        self.n[node].parent
    }

    fn previous(&self, mut node: usize) -> usize {
        if self.n[node].left != NIL {
            return self.back(self.n[node].left);
        }
        while self.n[node].parent != NIL && node == self.n[self.n[node].parent].left {
            node = self.n[node].parent;
        }
        self.n[node].parent
    }

    fn delete_node(&mut self, node: usize) {
        self.detach(node);
        self.n[node].right = self.free;
        self.free = node;
    }

    fn new_node(&mut self) -> usize {
        if self.free != NIL {
            let node = self.free;
            self.free = self.n[node].right;
            self.n[node].parent = NIL;
            self.n[node].left = NIL;
            self.n[node].right = NIL;
            self.n[node].red = true;
            return node;
        }
        self.n.push(RbNode {
            data: 0,
            parent: NIL,
            left: NIL,
            right: NIL,
            red: true,
        });
        self.n.len() - 1
    }
}

// ---------------------------------------------------------------- QTriangulator

const END_OF_POLYGON: u32 = u32::MAX;

struct Triangulator {
    vertices: Vec<Pt>,
    indices: Vec<u32>,
}

#[derive(Clone, Copy)]
struct CEdge {
    node: usize,
    from: i32,
    to: i32,
    next: i32,
    previous: i32,
    winding: i32,
    may_intersect: bool,
    pointing_up: bool,
    originally_pointing_up: bool,
}

impl CEdge {
    fn upper(&self) -> i32 {
        if self.pointing_up { self.to } else { self.from }
    }
    fn lower(&self) -> i32 {
        if self.pointing_up { self.from } else { self.to }
    }
}

#[derive(Clone, Copy)]
struct Intersection {
    point: IPoint,
    vertex: i32,
    left_edge: i32,
    right_edge: i32,
}

/// `Intersection::operator<`: the heap's top is the smallest point.
fn intersection_lt(a: &Intersection, b: &Intersection) -> bool {
    b.point.lt(&a.point)
}

#[derive(Clone, Copy)]
struct Split {
    vertex: i32,
    edge: i32,
    accurate: bool,
}

#[derive(Clone, Copy)]
struct CEvent {
    point: Pt,
    upper: bool,
    edge: i32,
}

struct ComplexToSimple<'a> {
    parent: &'a mut Triangulator,
    winding_fill: bool,
    edges: Vec<CEdge>,
    edge_list: RbTree,
    events: Vec<CEvent>,
    splits: Vec<Split>,
    top_intersection: MaxHeap<Intersection>,
    processed_edge_pairs: HashSet<u64>,
}

impl ComplexToSimple<'_> {
    fn v(&self, i: i32) -> Pt {
        self.parent.vertices[i as usize]
    }

    fn decompose(&mut self) {
        self.init_edges();
        loop {
            self.calculate_intersections();
            if !self.split_edges_at_intersections() {
                break;
            }
        }
        self.remove_unwanted_edges_and_connect();
        self.remove_unused_points();

        self.parent.indices.clear();
        let mut processed = vec![false; self.edges.len()];
        for first in 0..self.edges.len() {
            if processed[first] || self.edges[first].next == -1 {
                continue;
            }
            let mut i = first;
            loop {
                self.parent.indices.push(self.edges[i].from as u32);
                processed[i] = true;
                i = self.edges[i].next as usize;
                if i == first {
                    break;
                }
            }
            self.parent.indices.push(END_OF_POLYGON);
        }
    }

    fn init_edges(&mut self) {
        let mut first = 0;
        for i in 0..self.parent.indices.len() {
            if self.parent.indices[i] == END_OF_POLYGON {
                if self.edges.len() != first {
                    let f = self.edges[first].from;
                    self.edges.last_mut().unwrap().to = f;
                }
                first = self.edges.len();
            } else {
                self.edges.push(CEdge {
                    node: NIL,
                    from: self.parent.indices[i] as i32,
                    to: self.parent.indices[i + 1] as i32,
                    next: -1,
                    previous: -1,
                    winding: 0,
                    may_intersect: true,
                    pointing_up: false,
                    originally_pointing_up: false,
                });
            }
        }
        if first != self.edges.len() {
            let f = self.edges[first].from;
            self.edges.last_mut().unwrap().to = f;
        }
        for i in 0..self.edges.len() {
            let up = self.v(self.edges[i].to).lt(&self.v(self.edges[i].from));
            self.edges[i].originally_pointing_up = up;
            self.edges[i].pointing_up = up;
        }
    }

    /// True if a new intersection was found.
    fn calculate_intersection(&mut self, left: i32, right: i32) -> bool {
        let e1 = self.edges[left as usize];
        let e2 = self.edges[right as usize];
        let (u1, u2) = (self.v(e1.from), self.v(e1.to));
        let (v1, v2) = (self.v(e2.from), self.v(e2.to));
        if u1.x.max(u2.x) <= v1.x.min(v2.x) {
            return false;
        }
        let key = if left > right {
            ((right as u64) << 32) | left as u64
        } else {
            ((left as u64) << 32) | right as u64
        };
        if !self.processed_edge_pairs.insert(key) {
            return false;
        }
        let point = ipoint_of_lines(u1, u2, v1, v2);
        if !point.is_valid() {
            return false;
        }
        let it = Intersection {
            point,
            vertex: self.parent.vertices.len() as i32,
            left_edge: left,
            right_edge: right,
        };
        self.top_intersection.push(it, intersection_lt);
        self.parent.vertices.push(point.round());
        true
    }

    fn edge_is_left_of_edge(&self, left_edge_index: i32, right_edge_index: i32) -> bool {
        let left_edge = &self.edges[left_edge_index as usize];
        let right_edge = &self.edges[right_edge_index as usize];
        let u = self.v(right_edge.upper());
        let l = self.v(right_edge.lower());
        let upper = self.v(left_edge.upper());
        if upper.x < l.x.min(u.x) {
            return true;
        }
        if upper.x > l.x.max(u.x) {
            return false;
        }
        let mut d = point_distance_from_line(upper, l, u);
        if d == 0 {
            d = point_distance_from_line(self.v(left_edge.lower()), l, u);
        }
        d < 0
    }

    fn search_edge_left_of_after(&self, edge_index: i32, after: usize) -> usize {
        if self.edge_list.root == NIL {
            return after;
        }
        let mut result = after;
        let mut current = if after != NIL {
            self.edge_list.next(after)
        } else {
            self.edge_list.front(self.edge_list.root)
        };
        while current != NIL {
            if self.edge_is_left_of_edge(edge_index, self.edge_list.n[current].data) {
                return result;
            }
            result = current;
            current = self.edge_list.next(current);
        }
        result
    }

    /// Distance of `point` from the edge held by tree node `node`.
    fn node_dist(&self, point: Pt, node: usize) -> i64 {
        let e = &self.edges[self.edge_list.n[node].data as usize];
        point_distance_from_line(point, self.v(e.lower()), self.v(e.upper()))
    }

    fn bounds(&self, point: Pt) -> (usize, usize) {
        let t = &self.edge_list;
        let mut current = t.root;
        let mut result = (NIL, NIL);
        while current != NIL {
            let d = self.node_dist(point, current);
            if d == 0 {
                result = (current, current);
                break;
            }
            current = if d < 0 {
                t.n[current].left
            } else {
                t.n[current].right
            };
        }
        if current == NIL {
            return result;
        }
        current = t.n[result.0].left;
        while current != NIL {
            if self.node_dist(point, current) == 0 {
                result.0 = current;
                current = t.n[current].left;
            } else {
                current = t.n[current].right;
            }
        }
        current = t.n[result.1].right;
        while current != NIL {
            if self.node_dist(point, current) == 0 {
                result.1 = current;
                current = t.n[current].right;
            } else {
                current = t.n[current].left;
            }
        }
        result
    }

    fn outer_bounds(&self, point: Pt) -> (usize, usize) {
        let t = &self.edge_list;
        let mut current = t.root;
        let mut result = (NIL, NIL);
        while current != NIL {
            let d = self.node_dist(point, current);
            if d == 0 {
                break;
            }
            if d < 0 {
                result.1 = current;
                current = t.n[current].left;
            } else {
                result.0 = current;
                current = t.n[current].right;
            }
        }
        if current == NIL {
            return result;
        }
        let mid = current;
        current = t.n[mid].left;
        while current != NIL {
            if self.node_dist(point, current) == 0 {
                current = t.n[current].left;
            } else {
                result.0 = current;
                current = t.n[current].right;
            }
        }
        current = t.n[mid].right;
        while current != NIL {
            if self.node_dist(point, current) == 0 {
                current = t.n[current].right;
            } else {
                result.1 = current;
                current = t.n[current].left;
            }
        }
        result
    }

    fn split_edge_list_range(
        &mut self,
        mut leftmost: usize,
        rightmost: usize,
        vertex: i32,
        ip: &IPoint,
    ) {
        loop {
            let edge = self.edge_list.n[leftmost].data;
            let u = self.v(self.edges[edge as usize].from);
            let v = self.v(self.edges[edge as usize].to);
            if ip.x_offset.numerator != 0
                || ip.y_offset.numerator != 0
                || (ip.upper_left != u && ip.upper_left != v)
            {
                self.splits.push(Split {
                    vertex,
                    edge,
                    accurate: ip.is_accurate(),
                });
            }
            if leftmost == rightmost {
                break;
            }
            leftmost = self.edge_list.next(leftmost);
        }
    }

    fn reorder_edge_list_range(&mut self, mut leftmost: usize, mut rightmost: usize) {
        let store_leftmost = leftmost;
        let store_rightmost = rightmost;
        while leftmost != rightmost {
            let (ld, rd) = (
                self.edge_list.n[leftmost].data,
                self.edge_list.n[rightmost].data,
            );
            let (ln, rn) = (self.edges[ld as usize].node, self.edges[rd as usize].node);
            self.edges[ld as usize].node = rn;
            self.edges[rd as usize].node = ln;
            self.edge_list.n[leftmost].data = rd;
            self.edge_list.n[rightmost].data = ld;
            leftmost = self.edge_list.next(leftmost);
            if leftmost == rightmost {
                break;
            }
            rightmost = self.edge_list.previous(rightmost);
        }
        rightmost = self.edge_list.next(store_rightmost);
        leftmost = self.edge_list.previous(store_leftmost);
        if leftmost != NIL {
            let (a, b) = (
                self.edge_list.n[leftmost].data,
                self.edge_list.n[store_leftmost].data,
            );
            self.calculate_intersection(a, b);
        }
        if rightmost != NIL {
            let (a, b) = (
                self.edge_list.n[store_rightmost].data,
                self.edge_list.n[rightmost].data,
            );
            self.calculate_intersection(a, b);
        }
    }

    fn sort_edge_list(&mut self, event_point: Pt) {
        let event_point2 = ipoint_of(event_point);
        while !self.top_intersection.is_empty()
            && self.top_intersection.top().point.lt(&event_point2)
        {
            let intersection = self.top_intersection.pop(intersection_lt);
            let current_point = intersection.point;
            let current_vertex = intersection.vertex;
            let mut leftmost = self.edges[intersection.left_edge as usize].node;
            let mut rightmost = self.edges[intersection.right_edge as usize].node;
            loop {
                let previous = self.edge_list.previous(leftmost);
                if previous == NIL {
                    break;
                }
                let edge = &self.edges[self.edge_list.n[previous].data as usize];
                if !current_point.is_on_line(self.v(edge.from), self.v(edge.to)) {
                    break;
                }
                leftmost = previous;
            }
            loop {
                let next = self.edge_list.next(rightmost);
                if next == NIL {
                    break;
                }
                let edge = &self.edges[self.edge_list.n[next].data as usize];
                if !current_point.is_on_line(self.v(edge.from), self.v(edge.to)) {
                    break;
                }
                rightmost = next;
            }
            self.split_edge_list_range(leftmost, rightmost, current_vertex, &current_point);
            self.reorder_edge_list_range(leftmost, rightmost);
            // top <= current, i.e. !(current < top)
            while !self.top_intersection.is_empty()
                && !current_point.lt(&self.top_intersection.top().point)
            {
                self.top_intersection.pop(intersection_lt);
            }
        }
    }

    fn fill_priority_queue(&mut self) {
        self.events.clear();
        for i in 0..self.edges.len() {
            let e = self.edges[i];
            // Zero-length edges are ignored.
            if self.v(e.to) != self.v(e.from) {
                self.events.push(CEvent {
                    point: self.v(e.upper()),
                    upper: true,
                    edge: i as i32,
                });
                self.events.push(CEvent {
                    point: self.v(e.lower()),
                    upper: false,
                    edge: i as i32,
                });
            }
        }
        // Event::operator<: at one point Upper comes before Lower, else the greater point first.
        sort_by_lt(&mut self.events, |a, b| {
            if a.point == b.point {
                a.upper && !b.upper
            } else {
                b.point.lt(&a.point)
            }
        });
    }

    fn calculate_intersections(&mut self) {
        self.fill_priority_queue();
        while let Some(&last) = self.events.last() {
            let mut event = last;
            self.sort_edge_list(event.point);

            // The edges in the edge list that contain the current vertex are split later.
            let range = self.bounds(event.point);
            let left_node = if range.0 != NIL {
                self.edge_list.previous(range.0)
            } else {
                NIL
            };
            let e = &self.edges[event.edge as usize];
            let vertex = if event.upper { e.upper() } else { e.lower() };
            let event_point = ipoint_of(event.point);

            if range.0 != NIL {
                self.split_edge_list_range(range.0, range.1, vertex, &event_point);
                self.reorder_edge_list_range(range.0, range.1);
            }

            // The edges that start or end in the current vertex.
            while let Some(&l) = self.events.last() {
                if l.point != event.point {
                    break;
                }
                event = l;
                self.events.pop();
                let i = event.edge;
                let node = self.edges[i as usize].node;
                if node != NIL {
                    let left = self.edge_list.previous(node);
                    let right = self.edge_list.next(node);
                    self.edge_list.delete_node(node);
                    self.edges[i as usize].node = NIL;
                    if left == NIL || right == NIL {
                        continue;
                    }
                    let (a, b) = (self.edge_list.n[left].data, self.edge_list.n[right].data);
                    self.calculate_intersection(a, b);
                } else {
                    let left = self.search_edge_left_of_after(i, left_node);
                    let node = self.edge_list.new_node();
                    self.edges[i as usize].node = node;
                    self.edge_list.attach_after(left, node);
                    self.edge_list.n[node].data = i;
                    let right = self.edge_list.next(node);
                    if left != NIL {
                        let a = self.edge_list.n[left].data;
                        self.calculate_intersection(a, i);
                    }
                    if right != NIL {
                        let b = self.edge_list.n[right].data;
                        self.calculate_intersection(i, b);
                    }
                }
            }
            while !self.top_intersection.is_empty()
                && !event_point.lt(&self.top_intersection.top().point)
            {
                self.top_intersection.pop(intersection_lt);
            }
        }
        self.processed_edge_pairs.clear();
    }

    /// Splits an edge in two at the split's vertex: the upper piece goes to the end of `edges`,
    /// the lower one replaces the old edge.
    fn split_edge(&mut self, split_index: usize) {
        let split = self.splits[split_index];
        let lower = self.edges[split.edge as usize];
        if lower.from == split.vertex || lower.to == split.vertex {
            return;
        }
        let mut upper = lower;
        upper.may_intersect |= !split.accurate;
        let l = &mut self.edges[split.edge as usize];
        l.may_intersect = !split.accurate;
        if l.pointing_up {
            l.to = split.vertex;
            upper.from = split.vertex;
        } else {
            l.from = split.vertex;
            upper.to = split.vertex;
        }
        self.edges.push(upper);
    }

    fn split_edges_at_intersections(&mut self) -> bool {
        for e in &mut self.edges {
            e.may_intersect = false;
        }
        let mut check_for_new_intersections = false;
        for i in 0..self.splits.len() {
            self.split_edge(i);
            check_for_new_intersections |= !self.splits[i].accurate;
        }
        for i in 0..self.edges.len() {
            let up = self.v(self.edges[i].to).lt(&self.v(self.edges[i].from));
            self.edges[i].originally_pointing_up = up;
            self.edges[i].pointing_up = up;
        }
        self.splits.clear();
        check_for_new_intersections
    }

    fn insert_edge_into_vector_if_wanted(&self, ordered_edges: &mut Vec<i32>, i: i32) {
        let e = &self.edges[i as usize];
        let mut winding_number = e.winding;
        if e.originally_pointing_up {
            winding_number += 1;
        }
        if self.winding_fill && winding_number != 0 && winding_number != 1 {
            return;
        }
        // Cancelling edges are skipped.
        if let Some(&j) = ordered_edges.last() {
            let ej = &self.edges[j as usize];
            if ej.next == -1
                && ej.previous == -1
                && self.v(e.from) == self.v(ej.to)
                && self.v(e.to) == self.v(ej.from)
            {
                ordered_edges.pop();
                return;
            }
        }
        ordered_edges.push(i);
    }

    fn remove_unwanted_edges_and_connect(&mut self) {
        self.fill_priority_queue();
        let mut ordered_edges: Vec<i32> = Vec::new();
        while let Some(&last) = self.events.last() {
            let mut event = last;
            ordered_edges.clear();
            let b = self.outer_bounds(event.point);
            if self.edge_list.root != NIL {
                let mut current = if b.0 != NIL {
                    self.edge_list.next(b.0)
                } else {
                    self.edge_list.front(self.edge_list.root)
                };
                // The edges that leave the edge list at this event point.
                while current != b.1 {
                    let d = self.edge_list.n[current].data;
                    self.insert_edge_into_vector_if_wanted(&mut ordered_edges, d);
                    current = self.edge_list.next(current);
                }
            }

            // Edges above the event point are removed, edges below it inserted.
            loop {
                event = *self.events.last().unwrap();
                self.events.pop();
                let edge_index = event.edge;
                let node = self.edges[edge_index as usize].node;
                if node != NIL {
                    self.edge_list.delete_node(node);
                    self.edges[edge_index as usize].node = NIL;
                } else {
                    let left = self.search_edge_left_of_after(edge_index, b.0);
                    let node = self.edge_list.new_node();
                    self.edges[edge_index as usize].node = node;
                    self.edge_list.attach_after(left, node);
                    self.edge_list.n[node].data = edge_index;
                }
                match self.events.last() {
                    Some(l) if l.point == event.point => {}
                    _ => break,
                }
            }

            if self.edge_list.root != NIL {
                let mut current = if b.0 != NIL {
                    self.edge_list.next(b.0)
                } else {
                    self.edge_list.front(self.edge_list.root)
                };
                // Winding numbers; every edge is turned counter-clockwise.
                let mut current_winding_number = if b.0 != NIL {
                    self.edges[self.edge_list.n[b.0].data as usize].winding
                } else {
                    0
                };
                while current != b.1 {
                    let i = self.edge_list.n[current].data as usize;
                    let e = &mut self.edges[i];
                    e.winding = current_winding_number;
                    let mut ccw_winding_number = current_winding_number;
                    if e.originally_pointing_up {
                        e.winding -= 1;
                    } else {
                        e.winding += 1;
                        ccw_winding_number += 1;
                    }
                    current_winding_number = e.winding;
                    if ccw_winding_number & 1 == 0 {
                        std::mem::swap(&mut e.from, &mut e.to);
                        e.pointing_up = !e.pointing_up;
                    }
                    current = self.edge_list.next(current);
                }

                // The edges that entered the edge list at this event point.
                current = if b.1 != NIL {
                    self.edge_list.previous(b.1)
                } else {
                    self.edge_list.back(self.edge_list.root)
                };
                while current != b.0 {
                    let d = self.edge_list.n[current].data;
                    self.insert_edge_into_vector_if_wanted(&mut ordered_edges, d);
                    current = self.edge_list.previous(current);
                }
            }
            if ordered_edges.is_empty() {
                continue;
            }

            // Connect the edges; the first one must point towards the current point.
            let mut i;
            if self.v(self.edges[ordered_edges[0] as usize].from) == event.point {
                i = 1;
                let copy = ordered_edges[0];
                ordered_edges.push(copy);
            } else {
                i = 0;
            }

            // All edges reference the duplicate point with the lowest index.
            let mut point_index = i32::MAX;
            let mut j = i;
            while j + 1 < ordered_edges.len() {
                point_index = point_index
                    .min(self.edges[ordered_edges[j] as usize].to)
                    .min(self.edges[ordered_edges[j + 1] as usize].from);
                j += 2;
            }
            while i + 1 < ordered_edges.len() {
                let (a, b2) = (ordered_edges[i], ordered_edges[i + 1]);
                self.edges[a as usize].to = point_index;
                self.edges[b2 as usize].from = point_index;
                self.edges[a as usize].next = b2;
                self.edges[b2 as usize].previous = a;
                i += 2;
            }
        }
    }

    fn remove_unused_points(&mut self) {
        let n = self.parent.vertices.len();
        let mut used = vec![false; n];
        for e in &self.edges {
            if e.next != -1 {
                used[e.from as usize] = true;
            }
        }
        let mut new_mapping = vec![0i32; n];
        let mut count = 0;
        for i in 0..n {
            if used[i] {
                self.parent.vertices[count] = self.parent.vertices[i];
                new_mapping[i] = count as i32;
                count += 1;
            }
        }
        self.parent.vertices.truncate(count);
        for e in &mut self.edges {
            e.from = new_mapping[e.from as usize];
            e.to = new_mapping[e.to as usize];
        }
    }
}

/// libstdc++'s std::sort (introsort), so elements that compare equal end up where Qt has them.
pub(crate) fn sort_by_lt<T: Copy>(v: &mut [T], lt: impl Fn(&T, &T) -> bool) {
    fn linear_insert<T: Copy>(v: &mut [T], mut last: usize, lt: &impl Fn(&T, &T) -> bool) {
        let val = v[last];
        while last > 0 && lt(&val, &v[last - 1]) {
            v[last] = v[last - 1];
            last -= 1;
        }
        v[last] = val;
    }
    fn insertion_sort<T: Copy>(
        v: &mut [T],
        first: usize,
        last: usize,
        lt: &impl Fn(&T, &T) -> bool,
    ) {
        for i in first + 1..last {
            if lt(&v[i], &v[first]) {
                let val = v[i];
                v.copy_within(first..i, first + 1);
                v[first] = val;
            } else {
                let mut k = i;
                let val = v[k];
                while lt(&val, &v[k - 1]) {
                    v[k] = v[k - 1];
                    k -= 1;
                }
                v[k] = val;
            }
        }
    }
    fn heap_sort<T: Copy>(v: &mut [T], lt: &impl Fn(&T, &T) -> bool) {
        v.sort_by(|a, b| {
            if lt(a, b) {
                std::cmp::Ordering::Less
            } else if lt(b, a) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
    }
    fn introsort<T: Copy>(
        v: &mut [T],
        first: usize,
        mut last: usize,
        mut depth: u32,
        lt: &impl Fn(&T, &T) -> bool,
    ) {
        while last - first > 16 {
            if depth == 0 {
                heap_sort(&mut v[first..last], lt);
                return;
            }
            depth -= 1;
            // The median of the second, the middle and the last element becomes the pivot.
            let (a, b, c) = (first + 1, first + (last - first) / 2, last - 1);
            let m = if lt(&v[a], &v[b]) {
                if lt(&v[b], &v[c]) {
                    b
                } else if lt(&v[a], &v[c]) {
                    c
                } else {
                    a
                }
            } else if lt(&v[a], &v[c]) {
                a
            } else if lt(&v[b], &v[c]) {
                c
            } else {
                b
            };
            v.swap(first, m);
            let pivot = v[first];
            let (mut i, mut j) = (first + 1, last);
            let cut = loop {
                while lt(&v[i], &pivot) {
                    i += 1;
                }
                j -= 1;
                while lt(&pivot, &v[j]) {
                    j -= 1;
                }
                if i >= j {
                    break i;
                }
                v.swap(i, j);
                i += 1;
            };
            introsort(v, cut, last, depth, lt);
            last = cut;
        }
    }
    let n = v.len();
    if n == 0 {
        return;
    }
    introsort(v, 0, n, (usize::BITS - 1 - n.leading_zeros()) * 2, &lt);
    if n > 16 {
        insertion_sort(v, 0, 16, &lt);
        for i in 16..n {
            linear_insert(v, i, &lt);
        }
    } else {
        insertion_sort(v, 0, n, &lt);
    }
}

// ---------------------------------------------------------------- QTriangulator::SimpleToMonotone

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
enum VertexType {
    #[default]
    Merge,
    End,
    Regular,
    Start,
    Split,
}

#[derive(Clone, Copy)]
struct MEdge {
    node: usize,
    helper: i32,
    twin: i32,
    next: i32,
    previous: i32,
    from: i32,
    to: i32,
    ty: VertexType,
    pointing_up: bool,
}

impl MEdge {
    fn upper(&self) -> i32 {
        if self.pointing_up { self.to } else { self.from }
    }
    fn lower(&self) -> i32 {
        if self.pointing_up { self.from } else { self.to }
    }
}

struct SimpleToMonotone<'a> {
    parent: &'a mut Triangulator,
    edge_list: RbTree,
    edges: Vec<MEdge>,
    upper_vertex: Vec<i32>,
    clockwise_order: bool,
}

impl SimpleToMonotone<'_> {
    fn v(&self, i: i32) -> Pt {
        self.parent.vertices[i as usize]
    }

    fn decompose(&mut self) {
        self.setup_data_structures();
        self.remove_zero_length_edges();
        self.monotone_decomposition();

        self.parent.indices.clear();
        let mut processed = vec![false; self.edges.len()];
        for first in 0..self.edges.len() {
            if processed[first] {
                continue;
            }
            let mut i = first;
            loop {
                self.parent.indices.push(self.edges[i].from as u32);
                processed[i] = true;
                i = self.edges[i].next as usize;
                if i == first {
                    break;
                }
            }
            if self
                .parent
                .indices
                .last()
                .is_some_and(|&l| l != END_OF_POLYGON)
            {
                self.parent.indices.push(END_OF_POLYGON);
            }
        }
    }

    fn setup_data_structures(&mut self) {
        let mut i = 0;
        let ind = &self.parent.indices;
        while i + 3 <= ind.len() {
            let start = self.edges.len();
            loop {
                let n = self.edges.len() as i32;
                self.edges.push(MEdge {
                    node: NIL,
                    helper: -1,
                    twin: -1,
                    next: n + 1,
                    previous: n - 1,
                    from: ind[i] as i32,
                    to: 0,
                    ty: VertexType::Regular,
                    pointing_up: false,
                });
                i += 1;
                if ind[i] == END_OF_POLYGON {
                    break;
                }
            }
            let last = self.edges.len() - 1;
            self.edges[last].next = start as i32;
            self.edges[start].previous = last as i32;
            i += 1;
        }
        for i in 0..self.edges.len() {
            let to = self.edges[self.edges[i].next as usize].from;
            self.edges[i].to = to;
            self.edges[i].pointing_up = self.v(to).lt(&self.v(self.edges[i].from));
            self.edges[i].helper = -1;
        }
    }

    fn remove_zero_length_edges(&mut self) {
        for i in 0..self.edges.len() {
            let e = self.edges[i];
            if self.v(e.from) == self.v(e.to) {
                self.edges[e.previous as usize].next = e.next;
                self.edges[e.next as usize].previous = e.previous;
                self.edges[e.next as usize].from = e.from;
                self.edges[i].next = -1;
            }
        }
        let mut new_mapping = vec![0i32; self.edges.len()];
        let mut count = 0;
        for (i, mapped) in new_mapping.iter_mut().enumerate() {
            if self.edges[i].next != -1 {
                self.edges[count] = self.edges[i];
                *mapped = count as i32;
                count += 1;
            }
        }
        self.edges.truncate(count);
        for e in &mut self.edges {
            e.next = new_mapping[e.next as usize];
            e.previous = new_mapping[e.previous as usize];
        }
    }

    fn fill_priority_queue(&mut self) {
        let mut upper: Vec<i32> = (0..self.edges.len() as i32).collect();
        // CompareVertices: the greater point first; at one vertex the greater type first.
        sort_by_lt(&mut upper, |&i, &j| {
            let (a, b) = (&self.edges[i as usize], &self.edges[j as usize]);
            if a.from == b.from {
                a.ty > b.ty
            } else {
                self.v(b.from).lt(&self.v(a.from))
            }
        });
        self.upper_vertex = upper;
    }

    fn edge_is_left_of_edge(&self, left_edge_index: i32, right_edge_index: i32) -> bool {
        let left_edge = &self.edges[left_edge_index as usize];
        let right_edge = &self.edges[right_edge_index as usize];
        let u = self.v(right_edge.upper());
        let l = self.v(right_edge.lower());
        let mut d = point_distance_from_line(self.v(left_edge.upper()), l, u);
        if d == 0 {
            d = point_distance_from_line(self.v(left_edge.lower()), l, u);
        }
        d < 0
    }

    /// The rightmost edge not to the right of the given edge.
    fn search_edge_left_of_edge(&self, edge_index: i32) -> usize {
        let mut current = self.edge_list.root;
        let mut result = NIL;
        while current != NIL {
            if self.edge_is_left_of_edge(edge_index, self.edge_list.n[current].data) {
                current = self.edge_list.n[current].left;
            } else {
                result = current;
                current = self.edge_list.n[current].right;
            }
        }
        result
    }

    /// The rightmost edge left of the given point.
    fn search_edge_left_of_point(&self, point_index: i32) -> usize {
        let mut current = self.edge_list.root;
        let mut result = NIL;
        while current != NIL {
            let e = &self.edges[self.edge_list.n[current].data as usize];
            let d =
                point_distance_from_line(self.v(point_index), self.v(e.lower()), self.v(e.upper()));
            if d <= 0 {
                current = self.edge_list.n[current].left;
            } else {
                result = current;
                current = self.edge_list.n[current].right;
            }
        }
        result
    }

    fn classify_vertex(&mut self, i: usize) {
        let e2 = self.edges[i];
        let e1 = self.edges[e2.previous as usize];
        let start_or_split = e1.pointing_up && !e2.pointing_up;
        let end_or_merge = !e1.pointing_up && e2.pointing_up;
        let d = point_distance_from_line(self.v(e1.from), self.v(e2.from), self.v(e2.to));
        let inside = if self.clockwise_order { d < 0 } else { d > 0 };
        self.edges[i].ty = if start_or_split {
            if inside {
                VertexType::Split
            } else {
                VertexType::Start
            }
        } else if end_or_merge {
            if inside {
                VertexType::Merge
            } else {
                VertexType::End
            }
        } else {
            VertexType::Regular
        };
    }

    fn point_is_in_sector_pts(p: Pt, v1: Pt, v2: Pt, v3: Pt) -> bool {
        let left_of_previous_edge = !point_is_left_of_line(p, v2, v1);
        let left_of_next_edge = !point_is_left_of_line(p, v3, v2);
        if point_is_left_of_line(v1, v2, v3) {
            left_of_previous_edge && left_of_next_edge
        } else {
            left_of_previous_edge || left_of_next_edge
        }
    }

    fn point_is_in_sector(&self, mut vertex: i32, sector: i32) -> bool {
        let center = self.v(self.edges[sector as usize].from);
        // Degenerate edges.
        while self.v(self.edges[vertex as usize].from) == center {
            vertex = self.edges[vertex as usize].next;
        }
        let mut next = self.edges[sector as usize].next;
        while self.v(self.edges[next as usize].from) == center {
            next = self.edges[next as usize].next;
        }
        let mut previous = self.edges[sector as usize].previous;
        while self.v(self.edges[previous as usize].from) == center {
            previous = self.edges[previous as usize].previous;
        }
        let p = self.v(self.edges[vertex as usize].from);
        let v1 = self.v(self.edges[previous as usize].from);
        let v3 = self.v(self.edges[next as usize].from);
        if self.clockwise_order {
            Self::point_is_in_sector_pts(p, v3, center, v1)
        } else {
            Self::point_is_in_sector_pts(p, v1, center, v3)
        }
    }

    fn find_sector(&self, mut edge: i32, vertex: i32) -> i32 {
        while !self.point_is_in_sector(vertex, edge) {
            let twin = self.edges[self.edges[edge as usize].previous as usize].twin;
            if twin == -1 {
                break;
            }
            edge = twin;
        }
        edge
    }

    fn create_diagonal(&mut self, lower: i32, upper: i32) {
        let lower = self.find_sector(lower, upper);
        let upper = self.find_sector(upper, lower);
        let prev_lower = self.edges[lower as usize].previous;
        let prev_upper = self.edges[upper as usize].previous;
        let n = self.edges.len() as i32;
        let mut e = MEdge {
            node: NIL,
            helper: 0,
            twin: n + 1,
            next: upper,
            previous: prev_lower,
            from: self.edges[lower as usize].from,
            to: self.edges[upper as usize].from,
            ty: VertexType::Merge,
            pointing_up: false,
        };
        self.edges[upper as usize].previous = n;
        self.edges[prev_lower as usize].next = n;
        self.edges.push(e);

        e.twin = n;
        e.next = lower;
        e.previous = prev_upper;
        e.from = self.edges[upper as usize].from;
        e.to = self.edges[lower as usize].from;
        self.edges[lower as usize].previous = n + 1;
        self.edges[prev_upper as usize].next = n + 1;
        self.edges.push(e);
    }

    /// Whether edge `i`'s helper is a merge vertex.
    fn helper_is_merge(&self, i: usize) -> bool {
        let h = self.edges[i].helper;
        h >= 0 && self.edges[h as usize].ty == VertexType::Merge
    }

    fn monotone_decomposition(&mut self) {
        if self.edges.is_empty() {
            return;
        }
        let mut diagonals: Vec<(i32, i32)> = Vec::new();

        let mut i = 0;
        for index in 1..self.edges.len() {
            if self
                .v(self.edges[index].from)
                .lt(&self.v(self.edges[i].from))
            {
                i = index;
            }
        }
        let j = self.edges[i].previous as usize;
        self.clockwise_order = point_is_left_of_line(
            self.v(self.edges[i].from),
            self.v(self.edges[j].from),
            self.v(self.edges[i].to),
        );

        for k in 0..self.edges.len() {
            self.classify_vertex(k);
        }
        self.fill_priority_queue();

        while let Some(top) = self.upper_vertex.pop() {
            let i = top as usize;
            let j = self.edges[i].previous as usize;
            let ty = self.edges[i].ty;

            if ty == VertexType::Regular {
                // The polygon's interior is to the right of the vertex...
                if self.edges[i].pointing_up == self.clockwise_order {
                    if self.edges[i].node != NIL {
                        if self.helper_is_merge(i) {
                            diagonals.push((i as i32, self.edges[i].helper));
                        }
                        let node = self.edges[i].node;
                        self.edges[j].node = node;
                        self.edges[i].node = NIL;
                        self.edge_list.n[node].data = j as i32;
                        self.edges[j].helper = i as i32;
                    } else if self.edges[j].node != NIL {
                        if self.helper_is_merge(j) {
                            diagonals.push((i as i32, self.edges[j].helper));
                        }
                        let node = self.edges[j].node;
                        self.edges[i].node = node;
                        self.edges[j].node = NIL;
                        self.edge_list.n[node].data = i as i32;
                        self.edges[i].helper = i as i32;
                    }
                } else {
                    let left = self.search_edge_left_of_point(self.edges[i].from);
                    if left != NIL {
                        let l = self.edge_list.n[left].data as usize;
                        if self.helper_is_merge(l) {
                            diagonals.push((i as i32, self.edges[l].helper));
                        }
                        self.edges[l].helper = i as i32;
                    }
                }
                continue;
            }

            if ty == VertexType::Split {
                let left = self.search_edge_left_of_point(self.edges[i].from);
                if left != NIL {
                    let l = self.edge_list.n[left].data as usize;
                    diagonals.push((i as i32, self.edges[l].helper));
                    self.edges[l].helper = i as i32;
                }
            }
            if ty == VertexType::Split || ty == VertexType::Start {
                let k = if self.clockwise_order { j } else { i };
                let left = self.search_edge_left_of_edge(k as i32);
                let node = self.edge_list.new_node();
                self.edge_list.n[node].data = k as i32;
                self.edges[k].node = node;
                self.edges[k].helper = i as i32;
                self.edge_list.attach_after(left, node);
                continue;
            }

            if ty == VertexType::Merge {
                let left = self.search_edge_left_of_point(self.edges[i].from);
                if left != NIL {
                    let l = self.edge_list.n[left].data as usize;
                    if self.helper_is_merge(l) {
                        diagonals.push((i as i32, self.edges[l].helper));
                    }
                    self.edges[l].helper = i as i32;
                }
            }
            // Merge and End.
            let k = if self.clockwise_order { i } else { j };
            if self.helper_is_merge(k) {
                diagonals.push((i as i32, self.edges[k].helper));
            }
            if self.edges[k].node != NIL {
                self.edge_list.delete_node(self.edges[k].node);
                self.edges[k].node = NIL;
            }
        }

        for (a, b) in diagonals {
            if b >= 0 {
                self.create_diagonal(a, b);
            }
        }
    }
}

// ---------------------------------------------------------------- QTriangulator::MonotoneToTriangles

fn monotone_to_triangles(t: &mut Triangulator) {
    let mut result: Vec<u32> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut first = 0;
    while first + 3 <= t.indices.len() {
        let mut length = 0;
        while t.indices[first + length] != END_OF_POLYGON {
            length += 1;
        }
        if length < 3 {
            first += length + 1;
            continue;
        }
        let indices = |i: usize| t.indices[i + first];
        let next = |i: usize| (i + 1) % length;
        let previous = |i: usize| (i + length - 1) % length;
        let less = |i: usize, j: usize| {
            t.vertices[indices(i) as usize].lt(&t.vertices[indices(j) as usize])
        };
        let left_of_edge = |i: usize, j: usize, k: usize| {
            point_is_left_of_line(
                t.vertices[indices(i) as usize],
                t.vertices[indices(j) as usize],
                t.vertices[indices(k) as usize],
            )
        };

        let mut minimum = 0;
        while less(next(minimum), minimum) {
            minimum = next(minimum);
        }
        while less(previous(minimum), minimum) {
            minimum = previous(minimum);
        }

        stack.clear();
        stack.push(minimum);
        let mut left = previous(minimum);
        let mut right = next(minimum);
        let mut stack_is_on_left_side;
        let clockwise_order = left_of_edge(minimum, left, right);

        if less(left, right) {
            stack.push(left);
            left = previous(left);
            stack_is_on_left_side = true;
        } else {
            stack.push(right);
            right = next(right);
            stack_is_on_left_side = false;
        }

        let mut count = 0;
        while count + 2 < length {
            if less(left, right) {
                if !stack_is_on_left_side {
                    for i in 0..stack.len() - 1 {
                        result.extend([indices(stack[i + 1]), indices(left), indices(stack[i])]);
                    }
                    stack[0] = *stack.last().unwrap();
                    stack.truncate(1);
                } else {
                    while stack.len() >= 2
                        && (clockwise_order
                            ^ !left_of_edge(left, stack[stack.len() - 2], stack[stack.len() - 1]))
                    {
                        result.extend([
                            indices(stack[stack.len() - 2]),
                            indices(left),
                            indices(stack[stack.len() - 1]),
                        ]);
                        stack.pop();
                    }
                }
                stack.push(left);
                left = previous(left);
                stack_is_on_left_side = true;
            } else {
                if stack_is_on_left_side {
                    for i in 0..stack.len() - 1 {
                        result.extend([indices(stack[i]), indices(right), indices(stack[i + 1])]);
                    }
                    stack[0] = *stack.last().unwrap();
                    stack.truncate(1);
                } else {
                    while stack.len() >= 2
                        && (clockwise_order
                            ^ !left_of_edge(right, stack[stack.len() - 1], stack[stack.len() - 2]))
                    {
                        result.extend([
                            indices(stack[stack.len() - 1]),
                            indices(right),
                            indices(stack[stack.len() - 2]),
                        ]);
                        stack.pop();
                    }
                }
                stack.push(right);
                right = next(right);
                stack_is_on_left_side = false;
            }
            count += 1;
        }

        first += length + 1;
    }
    t.indices = result;
}

// ---------------------------------------------------------------- QPainterPath of lines + qTriangulate

/// A QPainterPath that only gets `moveTo` and `lineTo`.
pub struct LinePath {
    /// (is a moveTo, x, y)
    elements: Vec<(bool, f64, f64)>,
    c_start: usize,
    convex: bool,
}

impl Default for LinePath {
    fn default() -> Self {
        LinePath {
            elements: vec![(true, 0.0, 0.0)],
            c_start: 0,
            convex: false,
        }
    }
}

impl LinePath {
    pub fn move_to(&mut self, x: f64, y: f64) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        let last = self.elements.last_mut().unwrap();
        if last.0 {
            *last = (true, x, y);
        } else {
            self.elements.push((true, x, y));
        }
        self.c_start = self.elements.len() - 1;
    }

    pub fn line_to(&mut self, x: f64, y: f64) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        let last = *self.elements.last().unwrap();
        if (last.1, last.2) == (x, y) {
            return;
        }
        self.elements.push((false, x, y));
        let first = self.elements[self.c_start];
        let closed = (first.1, first.2) == (x, y);
        self.convex = self.elements.len() == 3 || (self.elements.len() == 4 && closed);
    }
}

/// qTriangulate(path): vertices `[x0, y0, x1, y1, ...]` and three indices for each triangle.
pub fn triangulate(path: &LinePath, winding_fill: bool) -> (Vec<f64>, Vec<u32>) {
    let mut t = Triangulator {
        vertices: Vec::new(),
        indices: Vec::new(),
    };
    // QVectorPathConverter: a path of alternating moveTo and lineTo is "lines", not an area.
    let is_lines = path
        .elements
        .iter()
        .enumerate()
        .all(|(i, e)| e.0 == (i % 2 == 0));
    let non_convex = !is_lines && !path.convex;
    for &(mv, x, y) in &path.elements {
        if mv && !t.indices.is_empty() {
            t.indices.push(END_OF_POLYGON);
        }
        t.indices.push(t.vertices.len() as u32);
        t.vertices.push(Pt {
            x: to_clamped_fixed_point(x),
            y: to_clamped_fixed_point(y),
        });
    }
    t.indices.push(END_OF_POLYGON);

    if non_convex {
        ComplexToSimple {
            parent: &mut t,
            winding_fill,
            edges: Vec::new(),
            edge_list: RbTree::new(),
            events: Vec::new(),
            splits: Vec::new(),
            top_intersection: MaxHeap::new(),
            processed_edge_pairs: HashSet::new(),
        }
        .decompose();
        SimpleToMonotone {
            parent: &mut t,
            edge_list: RbTree::new(),
            edges: Vec::new(),
            upper_vertex: Vec::new(),
            clockwise_order: false,
        }
        .decompose();
    }
    monotone_to_triangles(&mut t);

    let vertices = t
        .vertices
        .iter()
        .flat_map(|p| {
            [
                p.x as f64 / Q_FIXED_POINT_SCALE,
                p.y as f64 / Q_FIXED_POINT_SCALE,
            ]
        })
        .collect();
    (vertices, t.indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area2(v: &[f64], i: &[u32]) -> f64 {
        i.chunks(3)
            .map(|t| {
                let p = |k: usize| (v[t[k] as usize * 2], v[t[k] as usize * 2 + 1]);
                let (a, b, c) = (p(0), p(1), p(2));
                ((b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)).abs()
            })
            .sum()
    }

    #[test]
    fn a_square_is_two_triangles() {
        let mut p = LinePath::default();
        p.move_to(0.0, 0.0);
        p.line_to(10.0, 0.0);
        p.line_to(10.0, 10.0);
        p.line_to(0.0, 10.0);
        p.line_to(0.0, 0.0);
        let (v, i) = triangulate(&p, true);
        assert_eq!(i.len(), 6);
        assert_eq!(area2(&v, &i), 200.0);
    }

    #[test]
    fn a_concave_polygon_and_a_bow_tie_keep_their_area() {
        let mut p = LinePath::default();
        p.move_to(0.0, 0.0);
        for (x, y) in [
            (10.0, 0.0),
            (10.0, 10.0),
            (5.0, 4.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ] {
            p.line_to(x, y);
        }
        let (v, i) = triangulate(&p, true);
        assert_eq!(area2(&v, &i), 2.0 * (100.0 - 30.0));

        let mut p = LinePath::default();
        p.move_to(0.0, 0.0);
        for (x, y) in [(10.0, 10.0), (10.0, 0.0), (0.0, 10.0), (0.0, 0.0)] {
            p.line_to(x, y);
        }
        let (v, i) = triangulate(&p, false);
        assert_eq!(area2(&v, &i), 2.0 * 50.0);
    }

    #[test]
    fn a_hole_is_left_out() {
        let mut p = LinePath::default();
        p.move_to(0.0, 0.0);
        for (x, y) in [(10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)] {
            p.line_to(x, y);
        }
        p.move_to(3.0, 3.0);
        for (x, y) in [(3.0, 7.0), (7.0, 7.0), (7.0, 3.0), (3.0, 3.0)] {
            p.line_to(x, y);
        }
        let (v, i) = triangulate(&p, true);
        assert_eq!(area2(&v, &i), 2.0 * (100.0 - 16.0));
    }
}
