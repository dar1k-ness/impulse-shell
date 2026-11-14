//! QPathClipper (qtbase/src/gui/painting/qpathclipper.cpp), the general case of
//! QPainterPath::intersected: both paths become line segments (curves are flattened), every
//! crossing between them splits the segments into the edges of a winged-edge structure,
//! horizontal scan lines decide which side of which edge is inside both paths, and the
//! edges that bound the inside are walked into the answer. The names are Qt's.

use super::{D, El, Path, bez_at, pt_of};

/// fuzzyIsNull of the file, and qFuzzyIsNull(double).
fn fuzzy_is_null(d: f64) -> bool {
    d.abs() <= 1e-12
}

/// qFuzzyCompare(double, double).
fn fuzzy_compare(a: f64, b: f64) -> bool {
    (a - b).abs() * 1_000_000_000_000.0 <= a.abs().min(b.abs())
}

fn compare_points(a: D, b: D) -> bool {
    fuzzy_is_null(a[0] - b[0]) && fuzzy_is_null(a[1] - b[1])
}

fn dot(a: D, b: D) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn sub(a: D, b: D) -> D {
    [a[0] - b[0], a[1] - b[1]]
}

/// A QRectF: x, y, width, height. Its right and bottom are sums, as QRectF::right is.
#[derive(Clone, Copy, Default)]
struct Bounds([f64; 4]);

impl Bounds {
    fn left(&self) -> f64 {
        self.0[0]
    }
    fn top(&self) -> f64 {
        self.0[1]
    }
    fn right(&self) -> f64 {
        self.0[0] + self.0[2]
    }
    fn bottom(&self) -> f64 {
        self.0[1] + self.0[3]
    }
    fn apart(&self, o: &Bounds) -> bool {
        self.left() > o.right()
            || o.left() > self.right()
            || self.top() > o.bottom()
            || o.top() > self.bottom()
    }
    /// The low, middle and high coordinate on `axis`.
    fn span(&self, axis: usize) -> [f64; 3] {
        let (lo, len) = (self.0[axis], self.0[axis + 2]);
        [lo, lo + len / 2.0, lo + len]
    }
}

struct Segment {
    path: usize,
    va: usize,
    vb: usize,
    /// QPathSegments::Intersection of this segment, in the order they were found: where on
    /// the segment, and the point.
    intersections: Vec<(f64, usize)>,
    bounds: Bounds,
}

#[derive(Default)]
struct Segments {
    points: Vec<D>,
    segments: Vec<Segment>,
    path_id: usize,
}

/// The 3.14 QPathSegments::addPath counts the lines of a curve with.
#[expect(clippy::approx_constant, reason = "Qt's own constant, not pi")]
const QT_PI: f64 = 3.14;

fn is_line(b: &[D; 4]) -> bool {
    let equal_1_2 = compare_points(b[0], b[1]);
    let equal_2_3 = compare_points(b[1], b[2]);
    let equal_3_4 = compare_points(b[2], b[3]);
    if equal_1_2 && equal_2_3 && equal_3_4 {
        return true;
    }
    if compare_points(b[0], b[3]) {
        return equal_1_2 || equal_3_4;
    }
    (equal_1_2 && equal_3_4) || (equal_1_2 && equal_2_3) || (equal_2_3 && equal_3_4)
}

impl Segments {
    fn line_at(&self, i: usize) -> [D; 2] {
        let s = &self.segments[i];
        [self.points[s.va], self.points[s.vb]]
    }

    fn segment(&mut self, va: usize, vb: usize) {
        self.segments.push(Segment {
            path: self.path_id,
            va,
            vb,
            intersections: Vec::new(),
            bounds: Bounds::default(),
        });
    }

    /// QPathSegments::addPath.
    fn add_path(&mut self, path: &Path) {
        let first_segment = self.segments.len();
        let mut has_move_to = false;
        let mut last_move_to = 0;
        let mut last = 0;
        for (i, el) in path.el.iter().enumerate() {
            let mut current = self.points.len();
            let current_point = pt_of(el);
            if i > 0 && compare_points(self.points[last_move_to], current_point) {
                current = last_move_to;
            } else {
                self.points.push(current_point);
            }
            match *el {
                El::Move(_) => {
                    if has_move_to
                        && last != last_move_to
                        && !compare_points(self.points[last], self.points[last_move_to])
                    {
                        self.segment(last, last_move_to);
                    }
                    has_move_to = true;
                    last = current;
                    last_move_to = current;
                }
                El::Line(_) => {
                    self.segment(last, current);
                    last = current;
                }
                El::Cubic(c1, c2, e) => {
                    let bezier = [self.points[last], c1, c2, e];
                    if is_line(&bezier) {
                        self.segment(last, current);
                    } else {
                        // QBezier::bounds: of the control points.
                        let (mut lo, mut hi) = (bezier[0], bezier[0]);
                        for p in &bezier[1..] {
                            for k in 0..2 {
                                lo[k] = lo[k].min(p[k]);
                                hi[k] = hi[k].max(p[k]);
                            }
                        }
                        let size = (hi[0] - lo[0]).max(hi[1] - lo[1]);
                        // qMin<float>(64, ...): the product as a float, then as an int.
                        let mut threshold = ((size * (2.0 * QT_PI / 6.0)) as f32).min(64.0) as i32;
                        if threshold < 3 {
                            threshold = 3;
                        }
                        let one_over_threshold_minus_1 = 1.0 / f64::from(threshold - 1);
                        for t in 1..threshold - 1 {
                            let p = bez_at(&bezier, f64::from(t) * one_over_threshold_minus_1);
                            let index = self.points.len();
                            self.segment(last, index);
                            last = index;
                            self.points.push(p);
                        }
                        self.segment(last, current);
                    }
                    last = current;
                }
            }
        }
        if has_move_to
            && last != last_move_to
            && !compare_points(self.points[last], self.points[last_move_to])
        {
            self.segment(last, last_move_to);
        }
        for i in first_segment..self.segments.len() {
            let [a, b] = self.line_at(i);
            let (x1, x2) = if b[0] < a[0] {
                (b[0], a[0])
            } else {
                (a[0], b[0])
            };
            let (y1, y2) = if b[1] < a[1] {
                (b[1], a[1])
            } else {
                (a[1], b[1])
            };
            self.segments[i].bounds = Bounds([x1, y1, x2 - x1, y2 - y1]);
        }
        self.path_id += 1;
    }

    /// QPathSegments::mergePoints: points within fuzzyIsNull of each other become one.
    fn merge_points(&mut self) {
        if self.points.is_empty() {
            return;
        }
        let mut tree = KdPointTree::new(&self.points);
        let mut merged: Vec<D> = Vec::with_capacity(self.points.len());
        let mut indices = Vec::with_capacity(self.points.len());
        for &p in &self.points {
            let mut result = None;
            tree.find(tree.root, p, 0, &self.points, &mut result);
            let id = result.expect("the point itself is in the tree");
            if id >= merged.len() {
                merged.push(p);
            }
            indices.push(id);
        }
        for s in &mut self.segments {
            s.va = indices[s.va];
            s.vb = indices[s.vb];
            for (_, vertex) in &mut s.intersections {
                *vertex = indices[*vertex];
            }
        }
        self.points = merged;
    }
}

struct KdNode {
    point: usize,
    id: Option<usize>,
    left: Option<usize>,
    right: Option<usize>,
}

/// QKdPointTree.
struct KdPointTree {
    nodes: Vec<KdNode>,
    root: usize,
    next_id: usize,
}

impl KdPointTree {
    fn new(points: &[D]) -> Self {
        let nodes = (0..points.len())
            .map(|point| KdNode {
                point,
                id: None,
                left: None,
                right: None,
            })
            .collect();
        let mut tree = KdPointTree {
            nodes,
            root: 0,
            next_id: 0,
        };
        tree.root = tree.build(points, 0, points.len(), 0);
        tree
    }

    fn build(&mut self, points: &[D], begin: usize, end: usize, depth: usize) -> usize {
        let axis = depth & 1;
        let pivot = points[self.nodes[begin].point][axis];
        let mut first = begin + 1;
        let mut last = end - 1;
        while first <= last {
            if points[self.nodes[first].point][axis] < pivot {
                first += 1;
            } else {
                self.nodes.swap(first, last);
                last -= 1;
            }
        }
        if last != begin {
            self.nodes.swap(last, begin);
        }
        self.nodes[last].left = if last > begin {
            Some(self.build(points, begin, last, depth + 1))
        } else {
            None
        };
        self.nodes[last].right = if last + 1 < end {
            Some(self.build(points, last + 1, end, depth + 1))
        } else {
            None
        };
        last
    }

    /// qTraverseKdPointTree with a QKdPointFinder for `p`: the id of the first node found
    /// at `p`, which gets one if it has none.
    fn find(&mut self, node: usize, p: D, depth: usize, points: &[D], result: &mut Option<usize>) {
        if result.is_some() {
            return;
        }
        let at = points[self.nodes[node].point];
        let (pivot, value) = (at[depth & 1], p[depth & 1]);
        let (left, right) = if fuzzy_is_null(pivot - value) {
            if fuzzy_is_null(at[(depth + 1) & 1] - p[(depth + 1) & 1]) {
                let id = match self.nodes[node].id {
                    Some(id) => id,
                    None => {
                        self.next_id += 1;
                        self.next_id - 1
                    }
                };
                self.nodes[node].id = Some(id);
                *result = Some(id);
                return;
            }
            (true, true)
        } else {
            (value < pivot, value >= pivot)
        };
        if left && let Some(l) = self.nodes[node].left {
            self.find(l, p, depth + 1, points, result);
        }
        if right && let Some(r) = self.nodes[node].right {
            self.find(r, p, depth + 1, points, result);
        }
    }
}

/// QIntersection.
struct Crossing {
    alpha_a: f64,
    alpha_b: f64,
    pos: D,
}

/// SegmentTree::intersectLines.
fn intersect_lines(a: [D; 2], b: [D; 2], out: &mut Vec<Crossing>) {
    let [p1, p2] = a;
    let [q1, q2] = b;
    if compare_points(p1, p2) || compare_points(q1, q2) {
        return;
    }
    let p1_equals_q1 = compare_points(p1, q1);
    let p2_equals_q2 = compare_points(p2, q2);
    if p1_equals_q1 && p2_equals_q2 {
        return;
    }
    let p1_equals_q2 = compare_points(p1, q2);
    let p2_equals_q1 = compare_points(p2, q1);
    if p1_equals_q2 && p2_equals_q1 {
        return;
    }
    let p_delta = sub(p2, p1);
    let q_delta = sub(q2, q1);
    let par = p_delta[0] * q_delta[1] - p_delta[1] * q_delta[0];
    if fuzzy_is_null(par) {
        let normal = [-p_delta[1], p_delta[0]];
        // coinciding?
        if fuzzy_is_null(dot(normal, sub(q1, p1))) {
            let inv_dp = 1.0 / dot(p_delta, p_delta);
            let tq1 = dot(p_delta, sub(q1, p1)) * inv_dp;
            let tq2 = dot(p_delta, sub(q2, p1)) * inv_dp;
            if tq1 > 0.0 && tq1 < 1.0 {
                out.push(Crossing {
                    alpha_a: tq1,
                    alpha_b: 0.0,
                    pos: q1,
                });
            }
            if tq2 > 0.0 && tq2 < 1.0 {
                out.push(Crossing {
                    alpha_a: tq2,
                    alpha_b: 1.0,
                    pos: q2,
                });
            }
            let inv_dq = 1.0 / dot(q_delta, q_delta);
            let tp1 = dot(q_delta, sub(p1, q1)) * inv_dq;
            let tp2 = dot(q_delta, sub(p2, q1)) * inv_dq;
            if tp1 > 0.0 && tp1 < 1.0 {
                out.push(Crossing {
                    alpha_a: 0.0,
                    alpha_b: tp1,
                    pos: p1,
                });
            }
            if tp2 > 0.0 && tp2 < 1.0 {
                out.push(Crossing {
                    alpha_a: 1.0,
                    alpha_b: tp2,
                    pos: p2,
                });
            }
        }
        return;
    }
    // Lines that are not parallel and share an end point don't intersect.
    if p1_equals_q1 || p1_equals_q2 || p2_equals_q1 || p2_equals_q2 {
        return;
    }
    let tp = (q_delta[1] * (q1[0] - p1[0]) - q_delta[0] * (q1[1] - p1[1])) / par;
    let tq = (p_delta[1] * (q1[0] - p1[0]) - p_delta[0] * (q1[1] - p1[1])) / par;
    if !(0.0..=1.0).contains(&tp) || !(0.0..=1.0).contains(&tq) {
        return;
    }
    let p_zero = fuzzy_is_null(tp);
    let p_one = fuzzy_is_null(tp - 1.0);
    let q_zero = fuzzy_is_null(tq);
    let q_one = fuzzy_is_null(tq - 1.0);
    if (q_zero || q_one) && (p_zero || p_one) {
        return;
    }
    let pos = if p_zero {
        p1
    } else if p_one {
        p2
    } else if q_zero {
        q1
    } else if q_one {
        q2
    } else {
        [q1[0] + (q2[0] - q1[0]) * tq, q1[1] + (q2[1] - q1[1]) * tq]
    };
    out.push(Crossing {
        alpha_a: tp,
        alpha_b: tq,
        pos,
    });
}

#[derive(Clone, Copy, Default)]
struct TreeNode {
    split_left: f64,
    split_right: f64,
    leaf: bool,
    lowest_left_index: usize,
    lowest_right_index: usize,
    /// A leaf's first and last place in the index, or the left and right child.
    index: [usize; 2],
}

/// The corners of a rectangle: x1, y1, x2, y2.
type RectF = [f64; 4];

/// SegmentTree: the segments sorted into halves of their bounds, by x and y in turn.
struct SegmentTree {
    index: Vec<usize>,
    tree: Vec<TreeNode>,
}

impl SegmentTree {
    fn new(segments: &Segments) -> Self {
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for s in &segments.segments {
            let b = &s.bounds;
            bounds[0] = bounds[0].min(b.left());
            bounds[1] = bounds[1].min(b.top());
            bounds[2] = bounds[2].max(b.right());
            bounds[3] = bounds[3].max(b.bottom());
        }
        let mut tree = SegmentTree {
            index: (0..segments.segments.len()).collect(),
            tree: vec![TreeNode::default()],
        };
        let root = tree.build_tree(segments, 0, tree.index.len(), 0, bounds);
        tree.tree[0] = root;
        tree
    }

    fn build_tree(
        &mut self,
        segments: &Segments,
        first: usize,
        last: usize,
        depth: usize,
        bounds: RectF,
    ) -> TreeNode {
        if depth >= 24 || last - first <= 10 {
            return TreeNode {
                leaf: true,
                index: [first, last],
                ..TreeNode::default()
            };
        }
        let split_axis = depth & 1;
        let split = 0.5 * (bounds[split_axis] + bounds[split_axis + 2]);
        let tree_size = self.tree.len();
        let mut node = TreeNode {
            split_left: bounds[split_axis],
            split_right: bounds[split_axis + 2],
            leaf: false,
            lowest_left_index: usize::MAX,
            lowest_right_index: usize::MAX,
            index: [tree_size, tree_size + 1],
        };
        self.tree.resize(tree_size + 2, TreeNode::default());
        // Partition into left and right sets: `l` is the first not yet looked at, `r` one
        // past the last.
        let mut l = first;
        let mut r = last;
        while l < r {
            let index = self.index[l];
            let [low, center, high] = segments.segments[index].bounds.span(split_axis);
            if center < split {
                if high > node.split_left {
                    node.split_left = high;
                }
                if index < node.lowest_left_index {
                    node.lowest_left_index = index;
                }
                l += 1;
            } else {
                if low < node.split_right {
                    node.split_right = low;
                }
                if index < node.lowest_right_index {
                    node.lowest_right_index = index;
                }
                self.index.swap(l, r - 1);
                r -= 1;
            }
        }
        let mut lbounds = bounds;
        lbounds[split_axis + 2] = node.split_left;
        let mut rbounds = bounds;
        rbounds[split_axis] = node.split_right;
        let left = self.build_tree(segments, first, l, depth + 1, lbounds);
        self.tree[node.index[0]] = left;
        let right = self.build_tree(segments, l, last, depth + 1, rbounds);
        self.tree[node.index[1]] = right;
        node
    }

    /// The crossings of `segment` with the segments before it, added to both.
    fn produce_intersections(&self, segments: &mut Segments, segment: usize) {
        let b = segments.segments[segment].bounds;
        let sbounds = [b.left(), b.top(), b.right(), b.bottom()];
        self.produce(segments, self.tree[0], segment, sbounds, 0);
    }

    fn produce(
        &self,
        segments: &mut Segments,
        node: TreeNode,
        segment: usize,
        sbounds: RectF,
        axis: usize,
    ) {
        if node.leaf {
            self.produce_leaf(segments, node, segment);
            return;
        }
        if segment > node.lowest_left_index && sbounds[axis] <= node.split_left {
            self.produce(
                segments,
                self.tree[node.index[0]],
                segment,
                sbounds,
                axis ^ 1,
            );
        }
        if segment > node.lowest_right_index && sbounds[axis + 2] >= node.split_right {
            self.produce(
                segments,
                self.tree[node.index[1]],
                segment,
                sbounds,
                axis ^ 1,
            );
        }
    }

    fn produce_leaf(&self, segments: &mut Segments, node: TreeNode, segment: usize) {
        let r1 = segments.segments[segment].bounds;
        let line_a = segments.line_at(segment);
        let mut found = Vec::new();
        for &other in &self.index[node.index[0]..node.index[1]] {
            if other >= segment || r1.apart(&segments.segments[other].bounds) {
                continue;
            }
            found.clear();
            intersect_lines(line_a, segments.line_at(other), &mut found);
            for c in &found {
                let vertex = segments.points.len();
                segments.points.push(c.pos);
                segments.segments[segment]
                    .intersections
                    .push((c.alpha_a, vertex));
                segments.segments[other]
                    .intersections
                    .push((c.alpha_b, vertex));
            }
        }
    }
}

/// QPathEdge::Traversal.
const RIGHT: usize = 0;
const LEFT: usize = 1;
/// QPathEdge::Direction.
const FORWARD: usize = 0;
const BACKWARD: usize = 1;

bits::bit_flags! {
/// The bits of QPathEdge::flag: a side that was looked at, and a side that is in the answer.
const SEEN_LEFT: u32 = 1;
const SEEN_RIGHT: u32 = 2;
const IN_LEFT: u32 = 16;
const IN_RIGHT: u32 = 32;
}

struct Vertex {
    edge: Option<usize>,
    p: D,
}

struct Edge {
    flag: u32,
    winding_a: i32,
    winding_b: i32,
    first: usize,
    second: usize,
    angle: f64,
    inv_angle: f64,
    /// By traversal, then direction.
    next: [[usize; 2]; 2],
}

impl Edge {
    fn direction_to(&self, vertex: usize) -> usize {
        if self.first == vertex {
            BACKWARD
        } else {
            FORWARD
        }
    }
    fn vertex(&self, direction: usize) -> usize {
        if direction == BACKWARD {
            self.first
        } else {
            self.second
        }
    }
}

/// QWingedEdge::TraversalStatus.
#[derive(Clone, Copy)]
struct Status {
    edge: usize,
    traversal: usize,
    direction: usize,
}

impl Status {
    fn flip_direction(&mut self) {
        self.direction ^= 1;
    }
    fn flip(&mut self) {
        self.direction ^= 1;
        self.traversal ^= 1;
    }
}

fn compute_angle(v: D) -> f64 {
    if v[0] == 0.0 {
        return if v[1] <= 0.0 { 0.0 } else { 64.0 };
    } else if v[1] == 0.0 {
        return if v[0] <= 0.0 { 32.0 } else { 96.0 };
    }
    let reciprocal = 1.0 / (v[0] * v[0] + v[1] * v[1]).sqrt();
    let (vx, vy) = (v[0] * reciprocal, v[1] * reciprocal);
    if vy < 0.0 {
        if vx < 0.0 {
            -32.0 * vx
        } else {
            128.0 - 32.0 * vx
        }
    } else {
        64.0 + 32.0 * vx
    }
}

/// QWingedEdge.
struct WingedEdge {
    edges: Vec<Edge>,
    vertices: Vec<Vertex>,
}

impl WingedEdge {
    /// QWingedEdge(subject, clip) with intersectAndAdd.
    fn new(subject: &Path, clip: &Path) -> Self {
        let mut segments = Segments::default();
        segments.add_path(subject);
        segments.add_path(clip);
        let tree = SegmentTree::new(&segments);
        for i in 0..segments.segments.len() {
            tree.produce_intersections(&mut segments, i);
        }
        segments.merge_points();

        let mut list = WingedEdge {
            edges: Vec::new(),
            vertices: segments
                .points
                .iter()
                .map(|&p| Vertex { edge: None, p })
                .collect(),
        };
        for s in &mut segments.segments {
            // std::sort of a handful of crossings: equal places keep their order.
            s.intersections.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut last = s.va;
            for vertex in s
                .intersections
                .iter()
                .map(|i| i.1)
                .chain(std::iter::once(s.vb))
            {
                if let Some(e) = list.add_edge(last, vertex) {
                    let dir = if segments.points[last][1] < segments.points[vertex][1] {
                        1
                    } else {
                        -1
                    };
                    if s.path == 0 {
                        list.edges[e].winding_a += dir;
                    } else {
                        list.edges[e].winding_b += dir;
                    }
                }
                last = vertex;
            }
        }
        list
    }

    fn next(&self, status: Status) -> Status {
        let sp = &self.edges[status.edge];
        let mut result = Status {
            edge: sp.next[status.traversal][status.direction],
            ..status
        };
        if sp.vertex(status.direction) == self.edges[result.edge].vertex(status.direction) {
            result.flip();
        }
        result
    }

    fn delta(&self, vertex: usize, a: usize, b: usize) -> f64 {
        let (ap, bp) = (&self.edges[a], &self.edges[b]);
        let a_angle = if vertex == ap.second {
            ap.inv_angle
        } else {
            ap.angle
        };
        let b_angle = if vertex == bp.second {
            bp.inv_angle
        } else {
            bp.angle
        };
        let result = b_angle - a_angle;
        if result >= 128.0 {
            result - 128.0
        } else if result < 0.0 {
            result + 128.0
        } else {
            result
        }
    }

    fn find_insert_status(&self, vi: usize, ei: usize) -> Status {
        let start = self.vertices[vi].edge.expect("the vertex has an edge");
        let mut position = start;
        let mut d = 128.0;
        let mut status = Status {
            edge: start,
            traversal: RIGHT,
            direction: self.edges[start].direction_to(vi),
        };
        loop {
            status = self.next(status);
            status.flip();
            let d2 = self.delta(vi, ei, status.edge);
            if d2 < d {
                position = status.edge;
                d = d2;
            }
            if status.edge == start {
                break;
            }
        }
        let mut status = Status {
            edge: position,
            traversal: LEFT,
            direction: FORWARD,
        };
        if self.edges[status.edge].vertex(status.direction) != vi {
            status.flip();
        }
        status
    }

    /// commonEdge: the edge between vertices `a` and `b`, if there is one.
    fn common_edge(&self, a: usize, b: usize) -> Option<usize> {
        let start = self.vertices[a].edge?;
        self.vertices[b].edge?;
        let mut status = Status {
            edge: start,
            traversal: RIGHT,
            direction: self.edges[start].direction_to(a),
        };
        loop {
            let ep = &self.edges[status.edge];
            if (ep.first == a && ep.second == b) || (ep.first == b && ep.second == a) {
                return Some(status.edge);
            }
            status = self.next(status);
            status.flip();
            if status.edge == start {
                return None;
            }
        }
    }

    /// QWingedEdge::addEdge(int, int): `None` for an edge from a vertex to itself.
    fn add_edge(&mut self, fi: usize, si: usize) -> Option<usize> {
        if fi == si {
            return None;
        }
        if let Some(common) = self.common_edge(fi, si) {
            return Some(common);
        }
        let ei = self.edges.len();
        let tangent = sub(self.vertices[si].p, self.vertices[fi].p);
        let angle = compute_angle(tangent);
        let mut inv_angle = angle + 64.0;
        if inv_angle >= 128.0 {
            inv_angle -= 128.0;
        }
        self.edges.push(Edge {
            flag: 0,
            winding_a: 0,
            winding_b: 0,
            first: fi,
            second: si,
            angle,
            inv_angle,
            next: [[ei; 2]; 2],
        });
        for (vi, dir) in [(fi, BACKWARD), (si, FORWARD)] {
            if self.vertices[vi].edge.is_none() {
                self.vertices[vi].edge = Some(ei);
                self.edges[ei].next[RIGHT][dir] = ei;
                self.edges[ei].next[LEFT][dir] = ei;
            } else {
                let mut os = self.find_insert_status(vi, ei);
                let mut ns = self.next(os);
                ns.flip_direction();
                self.edges[os.edge].next[os.traversal][os.direction] = ei;
                self.edges[ns.edge].next[ns.traversal][ns.direction] = ei;
                let (oe, ne) = (os.edge, ns.edge);
                os = self.next(os);
                ns = self.next(ns);
                os.flip_direction();
                ns.flip_direction();
                self.edges[ei].next[os.traversal][os.direction] = oe;
                self.edges[ei].next[ns.traversal][ns.direction] = ne;
            }
        }
        Some(ei)
    }

    fn remove_edge(&mut self, ei: usize) {
        let mut status = Status {
            edge: ei,
            traversal: RIGHT,
            direction: FORWARD,
        };
        let mut forward_right = self.next(status);
        forward_right.flip_direction();
        status.traversal = LEFT;
        let mut forward_left = self.next(status);
        forward_left.flip_direction();
        status.direction = BACKWARD;
        let mut backward_left = self.next(status);
        backward_left.flip_direction();
        status.traversal = RIGHT;
        let mut backward_right = self.next(status);
        backward_right.flip_direction();

        let set = |list: &mut WingedEdge, at: Status, next: usize| {
            list.edges[at.edge].next[at.traversal][at.direction] = next;
        };
        set(self, forward_right, forward_left.edge);
        set(self, forward_left, forward_right.edge);
        set(self, backward_right, backward_left.edge);
        set(self, backward_left, backward_right.edge);

        let ep = &mut self.edges[ei];
        ep.next = [[ei; 2]; 2];
        let (first, second) = (ep.first, ep.second);
        self.vertices[first].edge = Some(backward_right.edge);
        self.vertices[second].edge = Some(forward_right.edge);
    }

    /// QWingedEdge::simplify: an edge with the inside on both sides is not a border.
    fn simplify(&mut self) {
        for i in 0..self.edges.len() {
            let flag = IN_LEFT | IN_RIGHT;
            if self.edges[i].flag & flag == flag {
                self.remove_edge(i);
                self.edges[i].flag &= !flag;
            }
        }
    }

    /// QWingedEdge::toPath.
    fn result_path(&mut self) -> Path {
        let mut path = Path::default();
        for i in 0..self.edges.len() {
            if self.edges[i].flag & IN_LEFT != 0 {
                self.add(&mut path, i, LEFT);
            }
            if self.edges[i].flag & IN_RIGHT != 0 {
                self.add(&mut path, i, RIGHT);
            }
        }
        path
    }

    /// add: the border that starts with `edge` on side `traversal`, as one subpath.
    fn add(&mut self, path: &mut Path, edge: usize, traversal: usize) {
        let mut status = Status {
            edge,
            traversal,
            direction: FORWARD,
        };
        path.p_move(self.vertices[self.edges[edge].first].p);
        loop {
            let ep = &mut self.edges[status.edge];
            ep.flag &= !(if status.traversal == LEFT {
                IN_LEFT
            } else {
                IN_RIGHT
            });
            let point = self.vertices[ep.vertex(status.direction)].p;
            add_line_to(path, point);
            status = self.next(status);
            if status.edge == edge {
                break;
            }
        }
    }

    /// traverse and clear: every side on the border that starts with `edge` on side
    /// `traversal` is marked as looked at, and with `inside` as part of the answer.
    fn mark(&mut self, edge: usize, traversal: usize, inside: bool) {
        let mut status = Status {
            edge,
            traversal,
            direction: FORWARD,
        };
        loop {
            let flag = if status.traversal == LEFT {
                SEEN_LEFT
            } else {
                SEEN_RIGHT
            };
            self.edges[status.edge].flag |= if inside { flag | (flag << 4) } else { flag };
            status = self.next(status);
            if status.edge == edge {
                break;
            }
        }
    }
}

/// addLineTo: a line that goes on straight from the last one extends it.
fn add_line_to(path: &mut Path, point: D) {
    if let [.., before, El::Line(middle)] = path.el[..] {
        let first = pt_of(&before);
        let d1 = sub(point, first);
        let d2 = sub(middle, first);
        if fuzzy_is_null(dot([-d1[1], d1[0]], d2)) {
            *path.el.last_mut().expect("matched above") = El::Line(point);
            return;
        }
    }
    path.p_line(point);
}

/// QPathClipper with the BoolAnd operation, after the answers QPathClipper::clip gives
/// without it. The masks pick the bits of a winding number that say inside: all of them
/// for the winding fill rule, the lowest for odd-even.
struct Clipper {
    a_mask: i32,
    b_mask: i32,
}

impl Clipper {
    /// QPathClipper::doClip in ClipMode.
    fn do_clip(&self, list: &mut WingedEdge) {
        let mut y_coords: Vec<f64> = list.vertices.iter().map(|v| v.p[1]).collect();
        y_coords.sort_by(f64::total_cmp);
        y_coords.dedup_by(|a, b| fuzzy_compare(*b, *a));
        let find = |from: usize, val: f64| {
            y_coords[from..]
                .iter()
                .position(|&y| fuzzy_compare(y, val))
                .map_or(y_coords.len(), |i| from + i)
        };
        loop {
            // The tallest edge with a side not yet looked at.
            let mut tallest = None;
            let mut max_height = 0.0;
            for (i, edge) in list.edges.iter().enumerate() {
                if edge.flag & 3 == 3 {
                    continue;
                }
                let (ay, by) = (
                    list.vertices[edge.first].p[1],
                    list.vertices[edge.second].p[1],
                );
                if fuzzy_compare(ay, by) {
                    continue;
                }
                let height = (ay - by).abs();
                if height > max_height {
                    tallest = Some(i);
                    max_height = height;
                }
            }
            let Some(index) = tallest else {
                break;
            };
            let edge = &list.edges[index];
            let (ay, by) = (
                list.vertices[edge.first].p[1],
                list.vertices[edge.second].p[1],
            );
            let first = find(0, ay.min(by));
            let last = find(first.min(y_coords.len()), ay.max(by));
            // Qt asserts both were found; a scan line can only be put between two of them.
            if first + 1 < y_coords.len() && last < y_coords.len() {
                let mut biggest_gap = y_coords[first + 1] - y_coords[first];
                let mut best_idx = first;
                for i in first + 1..last {
                    let gap = y_coords[i + 1] - y_coords[i];
                    if gap > biggest_gap {
                        best_idx = i;
                        biggest_gap = gap;
                    }
                }
                let best_y = 0.5 * (y_coords[best_idx] + y_coords[best_idx + 1]);
                self.handle_crossing_edges(list, best_y);
            }
            list.edges[index].flag |= 3;
        }
        list.simplify();
    }

    /// QPathClipper::handleCrossingEdges in ClipMode: along the line at `y`, left to right,
    /// each crossed edge has the area between it and the next one on one side; that side is
    /// inside the answer when it is inside both paths.
    fn handle_crossing_edges(&self, list: &mut WingedEdge, y: f64) {
        // findCrossings.
        let mut crossings: Vec<(usize, f64)> = Vec::new();
        for (i, edge) in list.edges.iter().enumerate() {
            let (a, b) = (list.vertices[edge.first].p, list.vertices[edge.second].p);
            if (a[1] < y && b[1] > y) || (a[1] > y && b[1] < y) {
                crossings.push((i, a[0] + (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1])));
            }
        }
        crossings.sort_by(|a, b| a.1.total_cmp(&b.1));
        let mut winding_a = 0;
        let mut winding_b = 0;
        let mut winding_d = 0;
        for &(ei, _) in crossings.iter().take(crossings.len().saturating_sub(1)) {
            let edge = &list.edges[ei];
            winding_a += edge.winding_a;
            winding_b += edge.winding_b;
            let has_left = edge.flag & IN_LEFT != 0;
            let has_right = edge.flag & IN_RIGHT != 0;
            winding_d += i32::from(has_left ^ has_right);
            let in_a = winding_a & self.a_mask != 0;
            let in_b = winding_b & self.b_mask != 0;
            let in_d = winding_d & 1 != 0;
            let inside = in_a && in_b;
            let add = in_d ^ inside;
            let flag = edge.flag;
            let upwards = list.vertices[edge.first].p[1] < list.vertices[edge.second].p[1];
            if add {
                // The side towards the next crossing is the left one of an edge that goes
                // to a larger y.
                if flag & SEEN_LEFT == 0 {
                    list.mark(ei, LEFT, upwards);
                }
                if list.edges[ei].flag & SEEN_RIGHT == 0 {
                    list.mark(ei, RIGHT, !upwards);
                }
                winding_d += 1;
            } else {
                if flag & SEEN_LEFT == 0 {
                    list.mark(ei, LEFT, false);
                }
                if list.edges[ei].flag & SEEN_RIGHT == 0 {
                    list.mark(ei, RIGHT, false);
                }
            }
        }
    }
}

/// QPathClipper(subject, clip).clip(BoolAnd) past its early answers: the area inside both
/// paths, as a path of lines with the odd-even fill rule.
pub(super) fn intersected(
    subject: &Path,
    even_odd: bool,
    clip: &Path,
    clip_even_odd: bool,
) -> Path {
    let clipper = Clipper {
        a_mask: if even_odd { 1 } else { !0 },
        b_mask: if clip_even_odd { 1 } else { !0 },
    };
    let mut list = WingedEdge::new(subject, clip);
    clipper.do_clip(&mut list);
    list.result_path()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn polygon(points: &[D]) -> Path {
        let mut p = Path::default();
        p.move_to(points[0][0], points[0][1]);
        for q in &points[1..] {
            p.line_to(q[0], q[1]);
        }
        p.close_subpath();
        p
    }

    /// The result holds a point exactly when both paths hold it, each under its own rule.
    fn check(a: &Path, a_even_odd: bool, b: &Path, b_even_odd: bool) {
        let both = intersected(a, a_even_odd, b, b_even_odd);
        let mut inside = 0;
        for y in 0..64 {
            for x in 0..64 {
                // Off the corners and edges of the shapes, which are on whole and half numbers.
                let p = [x as f64 * 0.5 + 0.13, y as f64 * 0.5 + 0.29];
                let want = a.contains(p, a_even_odd) && b.contains(p, b_even_odd);
                assert_eq!(both.contains(p, true), want, "{p:?}");
                inside += want as usize;
            }
        }
        assert!(inside > 0, "the shapes have no common area to test");
    }

    #[test]
    fn two_shapes_share_the_area_inside_both() {
        let triangle = polygon(&[[2.0, 3.0], [30.0, 14.0], [5.0, 29.0]]);
        let diamond = polygon(&[[16.0, 1.0], [31.0, 16.0], [16.0, 31.0], [1.0, 16.0]]);
        let star = polygon(&[
            [16.0, 1.0],
            [25.0, 30.0],
            [1.0, 12.0],
            [31.0, 12.0],
            [7.0, 30.0],
        ]);
        check(&triangle, false, &diamond, false);
        check(&diamond, false, &triangle, true);
        // The middle of the star is inside it by winding and outside it by odd-even.
        check(&star, false, &triangle, false);
        check(&star, true, &triangle, false);
        check(&triangle, false, &star, true);
        check(
            &star,
            true,
            &star.mapped([1.0, 0.0, 0.0, -1.0, 0.0, 32.0]),
            false,
        );
    }

    #[test]
    fn shapes_apart_share_nothing() {
        let a = polygon(&[[1.0, 1.0], [12.0, 1.0], [1.0, 12.0]]);
        let b = polygon(&[[13.0, 13.0], [13.0, 2.0], [2.0, 13.0]]);
        assert!(intersected(&a, false, &b, false).is_empty());
    }
}
