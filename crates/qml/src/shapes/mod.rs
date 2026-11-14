//! The singletons `Morph`, `RoundedPolygon` and `MaterialShapes` of the `Impulse` module
//! (`crates/qml/qmltypes/Impulse/`): Androidx's shape morphing, written from the script port the original
//! shipped (`morph.js` and the measuring and feature-mapping code it ran), operation for
//! operation so results are bit-identical to that script (`tests/impulse_libs.rs`).
//! Polygons built natively (`geom`, `material`) carry their features; polygons built in JS
//! have them read once and cached on the object.

use std::cell::OnceCell;
use std::rc::Rc;

use crate::eval::{Callable, Interp, NativeFn, R, fn_value, new_obj, set_own};
use crate::value::{Obj, Value};

mod geom;
mod js;
mod material;

pub use js::{MATERIAL_SHAPES, ROUNDED_POLYGON};

pub(crate) type Cubic = [f64; 8];

pub(crate) const DISTANCE_EPSILON: f64 = 1e-4;
const ANGLE_EPSILON: f64 = 1e-6;

pub(crate) fn interpolate(start: f64, stop: f64, fraction: f64) -> f64 {
    (1.0 - fraction) * start + fraction * stop
}

fn positive_modulo(v: f64, m: f64) -> f64 {
    ((v % m) + m) % m
}

/// `Math.min(a, b)` as the builtin computes it.
pub(crate) fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        f64::INFINITY.min(a).min(b)
    }
}

/// `Math.max(a, b)` as the builtin computes it.
pub(crate) fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        f64::NEG_INFINITY.max(a).max(b)
    }
}

pub(crate) fn point_on_curve(c: &Cubic, t: f64) -> (f64, f64) {
    let u = 1.0 - t;
    (
        c[0] * (u * u * u)
            + c[2] * (3.0 * t * u * u)
            + c[4] * (3.0 * t * t * u)
            + c[6] * (t * t * t),
        c[1] * (u * u * u)
            + c[3] * (3.0 * t * u * u)
            + c[5] * (3.0 * t * t * u)
            + c[7] * (t * t * t),
    )
}

pub(crate) fn split(c: &Cubic, t: f64) -> (Cubic, Cubic) {
    let u = 1.0 - t;
    let (px, py) = point_on_curve(c, t);
    (
        [
            c[0],
            c[1],
            c[0] * u + c[2] * t,
            c[1] * u + c[3] * t,
            c[0] * (u * u) + c[2] * (2.0 * u * t) + c[4] * (t * t),
            c[1] * (u * u) + c[3] * (2.0 * u * t) + c[5] * (t * t),
            px,
            py,
        ],
        [
            px,
            py,
            c[2] * (u * u) + c[4] * (2.0 * u * t) + c[6] * (t * t),
            c[3] * (u * u) + c[5] * (2.0 * u * t) + c[7] * (t * t),
            c[4] * u + c[6] * t,
            c[5] * u + c[7] * t,
            c[6],
            c[7],
        ],
    )
}

/// `LengthMeasurer.closestProgressTo`: (progress, measured length).
fn closest_progress_to(c: &Cubic, threshold: f64) -> (f64, f64) {
    const SEGMENTS: i32 = 3;
    let mut total = 0.0;
    let mut remainder = threshold;
    let (mut px, mut py) = (c[0], c[1]);
    for i in 1..SEGMENTS {
        let progress = i as f64 / SEGMENTS as f64;
        let (x, y) = point_on_curve(c, progress);
        let (dx, dy) = (x - px, y - py);
        let segment = (dx * dx + dy * dy).sqrt();
        if segment >= remainder {
            return (
                progress - (1.0 - remainder / segment) / SEGMENTS as f64,
                threshold,
            );
        }
        remainder -= segment;
        total += segment;
        px = x;
        py = y;
    }
    (1.0, total)
}

/// A polygon feature: its cubics, and `Some(convex)` for a corner.
#[derive(Clone, Debug)]
pub struct Feature {
    pub(crate) cubics: Vec<Cubic>,
    /// `Some(convex)` for a `Corner`, `None` for an `Edge`.
    pub(crate) corner: Option<bool>,
}

#[derive(Clone, Copy)]
struct MeasuredCubic {
    cubic: Cubic,
    start: f64,
    end: f64,
    size: f64,
}

impl MeasuredCubic {
    fn new(cubic: Cubic, start: f64, end: f64) -> Self {
        let size = closest_progress_to(&cubic, f64::INFINITY).1;
        MeasuredCubic {
            cubic,
            start,
            end,
            size,
        }
    }

    fn cut_at_progress(&self, cut: f64) -> (Self, Self) {
        let (lo, hi) = if self.start <= self.end {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        };
        let bounded = js_max(lo, js_min(hi, cut));
        let outline_size = self.end - self.start;
        let from_start = bounded - self.start;
        let relative = from_start / outline_size;
        let t = closest_progress_to(&self.cubic, relative * self.size).0;
        let (c1, c2) = split(&self.cubic, t);
        (
            Self::new(c1, self.start, bounded),
            Self::new(c2, bounded, self.end),
        )
    }
}

/// A corner feature at an outline progress, with what feature mapping reads of it.
#[derive(Clone, Copy)]
struct ProgressableFeature {
    progress: f64,
    convex: bool,
    x: f64,
    y: f64,
}

struct MeasuredPolygon {
    features: Vec<ProgressableFeature>,
    cubics: Vec<MeasuredCubic>,
}

impl MeasuredPolygon {
    fn new(
        it: &mut Interp,
        features: Vec<ProgressableFeature>,
        cubics: &[Cubic],
        outline: &[f64],
    ) -> R<Self> {
        let at = |i: usize| outline.get(i).copied().unwrap_or(f64::NAN);
        let mut measured: Vec<MeasuredCubic> = Vec::with_capacity(cubics.len());
        let mut start = 0.0;
        for (i, c) in cubics.iter().enumerate() {
            if at(i + 1) - at(i) > DISTANCE_EPSILON {
                measured.push(MeasuredCubic::new(*c, start, at(i + 1)));
                start = at(i + 1);
            }
        }
        let Some(last) = measured.last_mut() else {
            return it.throw(
                "TypeError",
                "Cannot read property 'updateProgressRange' of undefined",
            );
        };
        last.end = 1.0;
        Ok(MeasuredPolygon {
            features,
            cubics: measured,
        })
    }

    fn measure(it: &mut Interp, polygon: &[Feature]) -> R<Self> {
        let mut cubics = Vec::new();
        let mut feature_to_cubic = Vec::new();
        for f in polygon {
            for (ci, c) in f.cubics.iter().enumerate() {
                if let Some(convex) = f.corner
                    && ci as f64 == f.cubics.len() as f64 / 2.0
                {
                    feature_to_cubic.push((f, convex, cubics.len()));
                }
                cubics.push(*c);
            }
        }
        let mut measures = Vec::with_capacity(cubics.len() + 1);
        measures.push(0.0);
        for c in &cubics {
            let m = closest_progress_to(c, f64::INFINITY).1;
            if m < 0.0 {
                return it.throw(
                    "Error",
                    "Measured cubic is expected to be greater or equal to zero",
                );
            }
            let last = measures[measures.len() - 1];
            measures.push(last + m);
        }
        let total = measures[measures.len() - 1];
        let outline: Vec<f64> = measures.iter().map(|m| m / total).collect();
        let features = feature_to_cubic
            .iter()
            .map(|&(f, convex, ix)| {
                let first = &f.cubics[0];
                let last = &f.cubics[f.cubics.len() - 1];
                ProgressableFeature {
                    progress: positive_modulo((outline[ix] + outline[ix + 1]) / 2.0, 1.0),
                    convex,
                    x: (first[0] + last[6]) / 2.0,
                    y: (first[1] + last[7]) / 2.0,
                }
            })
            .collect();
        Self::new(it, features, &cubics, &outline)
    }

    /// `cutAndShift`; `None` means the polygon itself.
    fn cut_and_shift(&self, it: &mut Interp, cut: f64) -> R<Option<Self>> {
        if cut < DISTANCE_EPSILON {
            return Ok(None);
        }
        let n = self.cubics.len();
        let Some(ti) = self
            .cubics
            .iter()
            .position(|c| cut >= c.start && cut <= c.end)
        else {
            return it.throw(
                "TypeError",
                "Cannot read property 'cutAtProgress' of undefined",
            );
        };
        let (b1, b2) = self.cubics[ti].cut_at_progress(cut);
        let mut cubics = Vec::with_capacity(n + 1);
        cubics.push(b2.cubic);
        for i in 1..n {
            cubics.push(self.cubics[(i + ti) % n].cubic);
        }
        cubics.push(b1.cubic);
        let mut outline = Vec::with_capacity(n + 2);
        for i in 0..n + 2 {
            outline.push(if i == 0 {
                0.0
            } else if i == n + 1 {
                1.0
            } else {
                positive_modulo(self.cubics[(ti + i - 1) % n].end - cut, 1.0)
            });
        }
        let features = self
            .features
            .iter()
            .map(|f| ProgressableFeature {
                progress: positive_modulo(f.progress - cut, 1.0),
                ..*f
            })
            .collect();
        Self::new(it, features, &cubics, &outline).map(Some)
    }
}

fn progress_in_range(progress: f64, from: f64, to: f64) -> bool {
    if to >= from {
        progress >= from && progress <= to
    } else {
        progress >= from || progress <= to
    }
}

fn progress_distance(p1: f64, p2: f64) -> f64 {
    let it = (p1 - p2).abs();
    js_min(it, 1.0 - it)
}

fn linear_map(it: &mut Interp, xs: &[f64], ys: &[f64], x: f64) -> R<f64> {
    let n = xs.len();
    let Some(start) = (0..n).find(|&i| progress_in_range(x, xs[i], xs[(i + 1) % n])) else {
        return it.throw("Error", "No valid segment found");
    };
    let end = (start + 1) % n;
    let size_x = positive_modulo(xs[end] - xs[start], 1.0);
    let size_y = positive_modulo(ys[end] - ys[start], 1.0);
    let position = if size_x < 0.001 {
        0.5
    } else {
        positive_modulo(x - xs[start], 1.0) / size_x
    };
    Ok(positive_modulo(ys[start] + size_y * position, 1.0))
}

struct DoubleMapper {
    source: Vec<f64>,
    target: Vec<f64>,
}

impl DoubleMapper {
    fn map(&self, it: &mut Interp, x: f64) -> R<f64> {
        linear_map(it, &self.source, &self.target, x)
    }

    fn map_back(&self, it: &mut Interp, x: f64) -> R<f64> {
        linear_map(it, &self.target, &self.source, x)
    }
}

fn feature_mapper(
    it: &mut Interp,
    f1: &[ProgressableFeature],
    f2: &[ProgressableFeature],
) -> R<DoubleMapper> {
    let mut vertices = Vec::new();
    for (i, a) in f1.iter().enumerate() {
        for (j, b) in f2.iter().enumerate() {
            let d = if a.convex != b.convex {
                f64::MAX
            } else {
                let (x, y) = (a.x - b.x, a.y - b.y);
                x * x + y * y
            };
            if d != f64::MAX {
                vertices.push((d, i, j));
            }
        }
    }
    vertices.sort_by(|a, b| {
        (a.0 - b.0)
            .partial_cmp(&0.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mapping: Vec<(f64, f64)> = match vertices.len() {
        0 => vec![(0.0, 0.0), (0.5, 0.5)],
        1 => {
            let (p1, p2) = (f1[vertices[0].1].progress, f2[vertices[0].2].progress);
            vec![(p1, p2), ((p1 + 0.5) % 1.0, (p2 + 0.5) % 1.0)]
        }
        _ => {
            let mut mapping: Vec<(f64, f64)> = Vec::new();
            let (mut used1, mut used2) = (vec![false; f1.len()], vec![false; f2.len()]);
            for &(_, i, j) in &vertices {
                if used1[i] || used2[j] {
                    continue;
                }
                let (p1, p2) = (f1[i].progress, f2[j].progress);
                // The original's port of Kotlin's binarySearch: not found inserts at 0.
                let index = mapping
                    .iter()
                    .position(|m| m.0 == p1)
                    .map_or(-1, |k| k as i64);
                let ins = -index - 1;
                let n = mapping.len() as i64;
                if n >= 1 {
                    let (bi, ai) = ((ins + n - 1) % n, ins % n);
                    if bi < 0 || ai < 0 {
                        return it.throw("TypeError", "Cannot destructure undefined");
                    }
                    let ((b1, b2), (a1, a2)) = (mapping[bi as usize], mapping[ai as usize]);
                    if progress_distance(p1, b1) < DISTANCE_EPSILON
                        || progress_distance(p1, a1) < DISTANCE_EPSILON
                        || progress_distance(p2, b2) < DISTANCE_EPSILON
                        || progress_distance(p2, a2) < DISTANCE_EPSILON
                    {
                        continue;
                    }
                    if n > 1 && !progress_in_range(p2, b2, a2) {
                        continue;
                    }
                }
                let at = if ins < 0 {
                    (n + ins).max(0)
                } else {
                    ins.min(n)
                };
                mapping.insert(at as usize, (p1, p2));
                used1[i] = true;
                used2[j] = true;
            }
            mapping
        }
    };
    Ok(DoubleMapper {
        source: mapping.iter().map(|m| m.0).collect(),
        target: mapping.iter().map(|m| m.1).collect(),
    })
}

/// `Morph.match`: the pairs of cubics the morph interpolates between.
fn morph_match(it: &mut Interp, p1: &[Feature], p2: &[Feature]) -> R<Vec<(Cubic, Cubic)>> {
    let m1 = MeasuredPolygon::measure(it, p1)?;
    let m2 = MeasuredPolygon::measure(it, p2)?;
    let mapper = feature_mapper(it, &m1.features, &m2.features)?;
    let cut2 = mapper.map(it, 0.0)?;
    let shifted = m2.cut_and_shift(it, cut2)?;
    let bs1 = &m1.cubics;
    let bs2 = &shifted.as_ref().unwrap_or(&m2).cubics;
    let mut ret = Vec::new();
    let (mut i1, mut i2) = (1, 1);
    let mut b1 = bs1.first().copied();
    let mut b2 = bs2.first().copied();
    // The JS has no bound; the evaluator's step limit would end a runaway loop.
    let mut budget = 16 * (bs1.len() + bs2.len()) + 64;
    while let (Some(c1), Some(c2)) = (b1, b2) {
        budget -= 1;
        if budget == 0 {
            return it.throw("RangeError", "Morph.match did not converge");
        }
        let b1a = if i1 == bs1.len() { 1.0 } else { c1.end };
        let b2a = if i2 == bs2.len() {
            1.0
        } else {
            mapper.map_back(it, positive_modulo(c2.end + cut2, 1.0))?
        };
        let minb = js_min(b1a, b2a);
        let (seg1, nb1) = if b1a > minb + ANGLE_EPSILON {
            let (a, b) = c1.cut_at_progress(minb);
            (a, Some(b))
        } else {
            i1 += 1;
            (c1, bs1.get(i1 - 1).copied())
        };
        let (seg2, nb2) = if b2a > minb + ANGLE_EPSILON {
            let m = mapper.map(it, minb)?;
            let (a, b) = c2.cut_at_progress(positive_modulo(m - cut2, 1.0));
            (a, Some(b))
        } else {
            i2 += 1;
            (c2, bs2.get(i2 - 1).copied())
        };
        ret.push((seg1.cubic, seg2.cubic));
        b1 = nb1;
        b2 = nb2;
    }
    Ok(ret)
}

pub(crate) fn elems(it: &mut Interp, v: &Value) -> R<Vec<Value>> {
    if let Value::Obj(o) = v
        && let Some(a) = &o.borrow().array
    {
        return Ok(a.clone());
    }
    let n = it.get(v, "length")?.to_number();
    let mut out = Vec::new();
    for i in 0..n as usize {
        out.push(it.get(v, &i.to_string())?);
    }
    Ok(out)
}

/// A JS array of `Feature`s (`Corner`/`Edge` objects).
pub(crate) fn read_features(it: &mut Interp, fs: &Value) -> R<Vec<Feature>> {
    let mut out = Vec::new();
    for f in elems(it, fs)? {
        let cs = it.get(&f, "cubics")?;
        let mut cubics = Vec::new();
        for c in elems(it, &cs)? {
            let ps = it.get(&c, "points")?;
            let ps = elems(it, &ps)?;
            let mut cubic = [f64::NAN; 8];
            for (k, p) in ps.iter().take(8).enumerate() {
                cubic[k] = p.to_number();
            }
            cubics.push(cubic);
        }
        // `instanceof Corner`: corners set `isEdge = false`, edges `true`.
        let corner = match it.get(&f, "isEdge")? {
            Value::Bool(false) => Some(it.get(&f, "convex")?.truthy()),
            _ => None,
        };
        out.push(Feature { cubics, corner });
    }
    Ok(out)
}

/// The features of a JS `RoundedPolygon`, read once and cached on the object.
pub(crate) fn polygon_features(it: &mut Interp, v: &Value) -> R<Rc<Vec<Feature>>> {
    if let Value::Obj(o) = v
        && let Some(n) = &o.borrow().native
    {
        if let Some(p) = n.downcast_ref::<geom::Polygon>() {
            return Ok(p.features.clone());
        }
        if let Ok(f) = n.clone().downcast::<Vec<Feature>>() {
            return Ok(f);
        }
    }
    let fs = it.get(v, "features")?;
    let out = read_features(it, &fs)?;
    let out = Rc::new(out);
    if let Value::Obj(o) = v {
        o.borrow_mut().native = Some(out.clone());
    }
    Ok(out)
}

struct Protos {
    morph: Obj,
    cubic: Obj,
}

thread_local!(static PROTOS: OnceCell<Protos> = const { OnceCell::new() });

fn with_protos<T>(f: impl FnOnce(&Protos) -> T) -> T {
    PROTOS.with(|p| {
        f(p.get_or_init(|| {
            let morph = new_obj();
            set_own(
                &morph,
                "asCubics",
                fn_value(Callable::Native("asCubics", as_cubics)),
            );
            let cubic = new_obj();
            macro_rules! getters {
                ($($name:literal $i:literal),*) => {$(
                    crate::eval::define_accessor(
                        &cubic,
                        $name,
                        true,
                        fn_value(Callable::Native(
                            $name,
                            (|it: &mut Interp, this: &Value, _: &[Value]| point_of(it, this, $i))
                                as NativeFn,
                        )),
                        false,
                    );
                )*};
            }
            getters!("anchor0X" 0, "anchor0Y" 1, "control0X" 2, "control0Y" 3,
                     "control1X" 4, "control1Y" 5, "anchor1X" 6, "anchor1Y" 7);
            Protos { morph, cubic }
        }))
    })
}

/// `Cubic`'s `anchor0X`.. getters: `this.points[i]`.
fn point_of(it: &mut Interp, this: &Value, i: usize) -> R<Value> {
    let ps = it.get(this, "points")?;
    if let Value::Obj(o) = &ps
        && let Some(a) = &o.borrow().array
    {
        return Ok(a.get(i).cloned().unwrap_or_default());
    }
    it.get(&ps, &i.to_string())
}

pub(crate) fn cubic_value(c: &Cubic) -> Value {
    let o = new_obj();
    o.borrow_mut().proto = Some(with_protos(|p| p.cubic.clone()));
    set_own(
        &o,
        "points",
        Value::array(c.iter().map(|&x| Value::Num(x)).collect()),
    );
    Value::Obj(o)
}

fn morph_new(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let p1 = polygon_features(it, &a.first().cloned().unwrap_or_default())?;
    let p2 = polygon_features(it, &a.get(1).cloned().unwrap_or_default())?;
    let m = morph_match(it, &p1, &p2)?;
    let o = new_obj();
    {
        let mut b = o.borrow_mut();
        b.proto = Some(with_protos(|p| p.morph.clone()));
        b.native = Some(Rc::new(m));
    }
    Ok(Value::Obj(o))
}

fn as_cubics(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let m = match this {
        Value::Obj(o) => o
            .borrow()
            .native
            .clone()
            .and_then(|n| n.downcast::<Vec<(Cubic, Cubic)>>().ok()),
        _ => None,
    };
    let Some(m) = m else {
        return it.throw("TypeError", "asCubics called on a non-Morph");
    };
    let progress = a.first().map_or(f64::NAN, Value::to_number);
    let mut out = Vec::with_capacity(m.len());
    let mut first: Option<Cubic> = None;
    let mut last: Option<Cubic> = None;
    for (ca, cb) in m.iter() {
        let c: Cubic = std::array::from_fn(|k| interpolate(ca[k], cb[k], progress));
        if first.is_none() {
            first = Some(c);
        }
        if let Some(l) = last {
            out.push(cubic_value(&l));
        }
        last = Some(c);
    }
    if let (Some(l), Some(f)) = (last, first) {
        out.push(cubic_value(&[
            l[0], l[1], l[2], l[3], l[4], l[5], f[0], f[1],
        ]));
    }
    Ok(Value::array(out))
}

/// The `Morph` singleton of the `Impulse` module: `create(from, to)` matches two rounded
/// polygons, and the result's `asCubics(progress)` is the outline between them.
pub static MORPH: crate::impulse::Type = crate::impulse::Type {
    name: "Morph",
    methods: &[("create", morph_new)],
    init: None,
};
