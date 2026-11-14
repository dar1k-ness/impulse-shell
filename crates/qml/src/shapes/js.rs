//! What QML sees of `RoundedPolygon`, `Matrix` and `MaterialShapes`: the two singletons and
//! the polygon, matrix and point objects their methods return.

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

use super::geom::{self, Matrix, P, Polygon, Rounding, UNROUNDED, p};
use super::material::{self, GETTERS};
use super::{cubic_value, elems};
use crate::eval::{Callable, Interp, NativeFn, R, define_accessor, fn_value, new_obj, set_own};
use crate::value::{Obj, Value};

struct Protos {
    polygon: Obj,
    matrix: Obj,
}

thread_local! {
    static PROTOS: OnceCell<Protos> = const { OnceCell::new() };
    static SHAPES: RefCell<Vec<Option<Value>>> = RefCell::new(vec![None; GETTERS.len()]);
}

fn method(o: &Obj, name: &'static str, f: NativeFn) {
    set_own(o, name, fn_value(Callable::Native(name, f)));
}

fn getter(o: &Obj, name: &'static str, f: NativeFn) {
    define_accessor(o, name, true, fn_value(Callable::Native(name, f)), false);
}

fn with_protos<T>(f: impl FnOnce(&Protos) -> T) -> T {
    PROTOS.with(|p| {
        f(p.get_or_init(|| {
            let polygon = new_obj();
            method(&polygon, "transformed", poly_transformed);
            method(&polygon, "normalized", poly_normalized);
            method(&polygon, "calculateBounds", poly_bounds);
            method(&polygon, "calculateMaxBounds", poly_max_bounds);
            method(&polygon, "buildCubicList", poly_cubics);
            getter(&polygon, "features", poly_features);
            getter(&polygon, "cubics", poly_cubics_cached);
            getter(&polygon, "center", poly_center);
            getter(&polygon, "centerX", |_, this, _| {
                Ok(Value::Num(poly(this).center.x))
            });
            getter(&polygon, "centerY", |_, this, _| {
                Ok(Value::Num(poly(this).center.y))
            });
            let matrix = new_obj();
            method(&matrix, "map", matrix_map);
            method(&matrix, "get", matrix_get);
            getter(&matrix, "values", |_, this, _| {
                Ok(Value::array(
                    mat(this).0.iter().map(|&v| Value::Num(v)).collect(),
                ))
            });
            Protos { polygon, matrix }
        }))
    })
}

fn native<T: 'static>(v: &Value) -> Option<Rc<T>> {
    match v {
        Value::Obj(o) => o.borrow().native.clone()?.downcast::<T>().ok(),
        _ => None,
    }
}

fn poly(v: &Value) -> Rc<Polygon> {
    native::<Polygon>(v).unwrap_or_else(|| Rc::new(Polygon::new(Vec::new(), p(f64::NAN, f64::NAN))))
}

fn mat(v: &Value) -> Rc<Matrix> {
    native::<Matrix>(v).unwrap_or_else(|| Rc::new(Matrix::IDENTITY))
}

pub(crate) fn polygon_value(pg: Polygon) -> Value {
    let o = new_obj();
    {
        let mut b = o.borrow_mut();
        b.proto = Some(with_protos(|p| p.polygon.clone()));
        b.native = Some(Rc::new(pg));
    }
    Value::Obj(o)
}

fn matrix_value(m: Matrix) -> Value {
    let o = new_obj();
    {
        let mut b = o.borrow_mut();
        b.proto = Some(with_protos(|p| p.matrix.clone()));
        b.native = Some(Rc::new(m));
    }
    Value::Obj(o)
}

fn point_value(q: P) -> Value {
    let o = new_obj();
    set_own(&o, "x", Value::Num(q.x));
    set_own(&o, "y", Value::Num(q.y));
    Value::Obj(o)
}

fn rounding_value(rr: Rounding) -> Value {
    let o = new_obj();
    set_own(&o, "radius", Value::Num(rr.radius));
    set_own(&o, "smoothing", Value::Num(rr.smoothing));
    Value::Obj(o)
}

fn nums(v: &[f64]) -> Value {
    Value::array(v.iter().map(|&x| Value::Num(x)).collect())
}

fn arg(a: &[Value], i: usize) -> Value {
    a.get(i).cloned().unwrap_or_default()
}

/// A number argument with its JS default for `undefined`.
fn num(a: &[Value], i: usize, default: f64) -> f64 {
    match a.get(i) {
        None | Some(Value::Undefined) => default,
        Some(v) => v.to_number(),
    }
}

fn nullish(v: &Value) -> bool {
    matches!(v, Value::Undefined | Value::Null)
}

fn read_point(it: &mut Interp, v: &Value) -> R<P> {
    Ok(p(it.get(v, "x")?.to_number(), it.get(v, "y")?.to_number()))
}

/// A `CornerRounding`; `null`/`undefined` read as no rounding (`rounding?.radius ?? 0`).
fn read_rounding(it: &mut Interp, v: &Value) -> R<Option<Rounding>> {
    if nullish(v) {
        return Ok(None);
    }
    let mut f = |k| -> R<f64> {
        let x = it.get(v, k)?;
        Ok(if nullish(&x) { 0.0 } else { x.to_number() })
    };
    Ok(Some(geom::r(f("radius")?, f("smoothing")?)))
}

fn read_per_vertex(it: &mut Interp, v: &Value) -> R<Option<Vec<Option<Rounding>>>> {
    if nullish(v) {
        return Ok(None);
    }
    let mut out = Vec::new();
    for x in elems(it, v)? {
        out.push(read_rounding(it, &x)?);
    }
    Ok(Some(out))
}

fn poly_transformed(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let f = arg(a, 0);
    let pg = poly(this).transformed(|x, y| {
        let q = it.call(&f, Value::Undefined, &[Value::Num(x), Value::Num(y)])?;
        read_point(it, &q)
    })?;
    Ok(polygon_value(pg))
}

fn poly_normalized(_: &mut Interp, this: &Value, _: &[Value]) -> R<Value> {
    Ok(polygon_value(poly(this).normalized()))
}

/// Writes `b` into the caller's `bounds` array (as the JS does) or a new one.
fn fill_bounds(it: &mut Interp, bounds: &Value, b: [f64; 4]) -> R<Value> {
    if !matches!(bounds, Value::Obj(_)) {
        return Ok(nums(&b));
    }
    for (i, v) in b.iter().enumerate() {
        it.put(bounds, &i.to_string(), Value::Num(*v))?;
    }
    Ok(bounds.clone())
}

fn poly_bounds(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let approximate = match a.get(1) {
        None | Some(Value::Undefined) => true,
        Some(v) => v.truthy(),
    };
    let b = poly(this).bounds(approximate);
    fill_bounds(it, &arg(a, 0), b)
}

fn poly_max_bounds(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let b = poly(this).max_bounds();
    fill_bounds(it, &arg(a, 0), b)
}

fn poly_cubics(_: &mut Interp, this: &Value, _: &[Value]) -> R<Value> {
    Ok(Value::array(
        poly(this).cubics.iter().map(cubic_value).collect(),
    ))
}

/// Materializes a JS view once and keeps it as an own property, so it stays the same object.
fn cache_own(this: &Value, k: &str, v: Value) -> Value {
    if let Value::Obj(o) = this {
        set_own(o, k, v.clone());
    }
    v
}

fn poly_cubics_cached(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let v = poly_cubics(it, this, a)?;
    Ok(cache_own(this, "cubics", v))
}

fn poly_center(_: &mut Interp, this: &Value, _: &[Value]) -> R<Value> {
    Ok(cache_own(this, "center", point_value(poly(this).center)))
}

fn poly_features(_: &mut Interp, this: &Value, _: &[Value]) -> R<Value> {
    let fs = poly(this)
        .features
        .iter()
        .map(|f| {
            let o = new_obj();
            let cubics = Value::array(f.cubics.iter().map(cubic_value).collect());
            set_own(&o, "cubics", cubics);
            let (edge, convex) = match f.corner {
                Some(c) => {
                    set_own(&o, "convex", Value::Bool(c));
                    (false, c)
                }
                None => (true, false),
            };
            set_own(&o, "isIgnorableFeature", Value::Bool(edge));
            set_own(&o, "isEdge", Value::Bool(edge));
            set_own(&o, "isConvexCorner", Value::Bool(!edge && convex));
            set_own(&o, "isConcaveCorner", Value::Bool(!edge && !convex));
            Value::Obj(o)
        })
        .collect();
    Ok(cache_own(this, "features", Value::array(fs)))
}

fn matrix_map(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let q = read_point(it, &arg(a, 0))?;
    Ok(point_value(mat(this).map(q.x, q.y)))
}

fn matrix_get(_: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let i = num(a, 0, f64::NAN) * 4.0 + num(a, 1, f64::NAN);
    let m = mat(this);
    Ok(if (0.0..16.0).contains(&i) && i.fract() == 0.0 {
        Value::Num(m.0[i as usize])
    } else {
        Value::Undefined
    })
}

fn rp_star(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let rr = read_rounding(it, &arg(a, 3))?.unwrap_or(UNROUNDED);
    let pv = read_per_vertex(it, &arg(a, 5))?;
    let (n, radius, inner) = (num(a, 0, f64::NAN), num(a, 1, 1.0), num(a, 2, 0.5));
    let pg = geom::star(
        n,
        radius,
        inner,
        rr,
        pv.as_deref(),
        num(a, 6, 0.0),
        num(a, 7, 0.0),
    );
    Ok(polygon_value(pg))
}

/// The `RoundedPolygon` singleton of the `Impulse` module: the builders of rounded polygons.
pub static ROUNDED_POLYGON: crate::impulse::Type = crate::impulse::Type {
    name: "RoundedPolygon",
    methods: &[("star", rp_star)],
    init: None,
};

/// `getX()`: built once per thread, then the same object (like the JS `_x` caches).
fn cached(i: usize) -> Value {
    if let Some(v) = SHAPES.with(|s| s.borrow()[i].clone()) {
        return v;
    }
    let v = polygon_value(material::build(GETTERS[i].1).expect("material shape"));
    SHAPES.with(|s| s.borrow_mut()[i] = Some(v.clone()));
    v
}

fn ms_custom_polygon(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let mut pnr = Vec::new();
    for x in elems(it, &arg(a, 0))? {
        let o = it.get(&x, "o")?;
        let o = read_point(it, &o)?;
        let rr = it.get(&x, "r")?;
        pnr.push((o, read_rounding(it, &rr)?.unwrap_or(UNROUNDED)));
    }
    let center = match a.get(2) {
        None | Some(Value::Undefined) => p(0.5, 0.5),
        Some(c) => read_point(it, c)?,
    };
    let mirroring = a.get(3).is_some_and(Value::truthy);
    Ok(polygon_value(material::custom_polygon(
        &pnr,
        num(a, 1, 1.0),
        center,
        mirroring,
    )))
}

fn ms_point_n_round(_: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let o = new_obj();
    set_own(&o, "o", arg(a, 0));
    let rr = match a.get(1) {
        None | Some(Value::Undefined) => rounding_value(UNROUNDED),
        Some(v) => v.clone(),
    };
    set_own(&o, "r", rr);
    Ok(Value::Obj(o))
}

macro_rules! shape_methods {
    ($($i:literal)*) => {
        &[
            $(
                (GETTERS[$i].0, |_, _, _| Ok(cached($i))),
                (GETTERS[$i].1, |_, _, _| {
                    Ok(polygon_value(material::build(GETTERS[$i].1).expect("material shape")))
                }),
            )*
            ("customPolygon", ms_custom_polygon),
            ("pointNRound", ms_point_n_round),
        ]
    };
}

/// The `MaterialShapes` singleton of the `Impulse` module: the Material 3 shapes, their
/// rotation matrices and corner roundings.
pub static MATERIAL_SHAPES: crate::impulse::Type = crate::impulse::Type {
    name: "MaterialShapes",
    methods: shape_methods!(0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34),
    init: Some(|w, id| {
        for (name, rr) in [
            ("cornerRound15", material::R15),
            ("cornerRound20", material::R20),
            ("cornerRound30", material::R30),
            ("cornerRound50", material::R50),
            ("cornerRound100", material::R100),
        ] {
            w.write(id, name, rounding_value(rr));
        }
        for (name, m) in material::matrices() {
            w.write(id, name, matrix_value(m));
        }
    }),
};
