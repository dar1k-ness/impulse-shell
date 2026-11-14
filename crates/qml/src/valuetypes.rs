//! The structured QML value types of QtQuick (Qt 6.12, `qquickvaluetypes_p.h`): `font`,
//! `matrix4x4`, `quaternion` and `vector2d`/`vector3d`/`vector4d`, those of QtQml
//! (`qqmlvaluetype_p.h`): `point`, `size` and `rect`, and the `url` variant.
//!
//! A value type is an object whose own data properties are the type's `Q_PROPERTY`s (stored
//! as the C++ type holds them: `float` for the math types, `qreal` for the geometry ones), with no `[[Prototype]]` of its
//! own: [`crate::eval::Interp`] gives it the per-type prototype of [`Protos`], which (as
//! QQmlValueTypeWrapper's) only has `toString`. The `Q_INVOKABLE` methods are resolved on
//! the object itself (QQmlValueTypeWrapper::virtualGet), each read giving a new function
//! bound to the value. They are not own properties and reflection does not list them, but
//! `in` finds them like a read does (Qt 6.12 answers `false` there).

use std::rc::Rc;

use crate::eval::{new_obj, set_hidden};
use crate::value::{Internal, JsObject, Obj, Value};

/// A value type, with the state of a `font` that its properties do not show.
/// What a QFont holds that no QML font property shows: only a QFont::toString text sets
/// it, and the text of the font prints it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontRaw {
    /// QFont::style() when it is neither StyleNormal nor StyleItalic (`italic` is then true).
    style: u8,
    fixed_pitch: bool,
    /// QFont::styleHint().
    hint: u8,
    /// QFont::stretch().
    stretch: u16,
    /// QFont::styleStrategy() without PreferNoShaping, ContextFontMerging and
    /// PreferTypoLineMetrics, which are properties.
    strategy: u16,
}

impl FontRaw {
    /// AnyStyle, AnyStretch, PreferDefault.
    pub const DEFAULT: FontRaw = FontRaw {
        style: 0,
        fixed_pitch: false,
        hint: 5,
        stretch: 0,
        strategy: 1,
    };
}

/// QFont::PreferNoShaping, ContextFontMerging and PreferTypoLineMetrics.
const STRATEGY_PROPS: [(&str, u16); 3] = [
    ("preferShaping", 0x1000),
    ("contextFontMerging", 0x2000),
    ("preferTypoLineMetrics", 0x4000),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vt {
    Font {
        /// The size was last set in pixels (QFont::pixelSize() != -1).
        pixel_sized: bool,
        /// `letterSpacing` was set (QFont::letterSpacingType() is AbsoluteSpacing).
        spacing: bool,
        raw: FontRaw,
    },
    Matrix4x4 {
        /// QMatrix4x4::flagBits: what the matrix is known to hold.
        flags: u8,
    },
    Quaternion,
    Vector2D,
    Vector3D,
    Vector4D,
    PointF,
    SizeF,
    RectF,
    /// QSize and QRect: the properties of the `F` types, held as `int`s.
    Size,
    Rect,
}

pub const MATRIX_FIELDS: [&str; 16] = [
    "m11", "m12", "m13", "m14", "m21", "m22", "m23", "m24", "m31", "m32", "m33", "m34", "m41",
    "m42", "m43", "m44",
];

pub const FONT_FIELDS: [&str; 20] = [
    "family",
    "styleName",
    "bold",
    "weight",
    "italic",
    "underline",
    "overline",
    "strikeout",
    "pointSize",
    "pixelSize",
    "capitalization",
    "letterSpacing",
    "wordSpacing",
    "hintingPreference",
    "kerning",
    "preferShaping",
    "features",
    "variableAxes",
    "contextFontMerging",
    "preferTypoLineMetrics",
];

impl Vt {
    pub const FONT: Vt = Vt::Font {
        pixel_sized: false,
        spacing: false,
        raw: FontRaw::DEFAULT,
    };

    /// The C++ type of the type's `Q_PROPERTY` `field`.
    pub fn field_type(self, field: &str) -> String {
        match (self.cpp(), field) {
            ("QFont", "family" | "styleName") => "QString".into(),
            ("QFont", "pixelSize" | "weight") => "int".into(),
            ("QFont", "pointSize" | "letterSpacing" | "wordSpacing") => "double".into(),
            ("QFont", "capitalization" | "hintingPreference") => {
                let mut f = field.to_string();
                f[..1].make_ascii_uppercase();
                format!("QFont::{f}")
            }
            ("QFont", _) => "bool".into(),
            ("QSize" | "QRect", _) => "int".into(),
            _ if self.real() => "double".into(),
            _ => "float".into(),
        }
    }

    /// What the type's setter keeps of `v` written to `field`, `None` when it keeps what it
    /// has: QFont::setWeight bounds a weight, a size that is not above 0 is no size
    /// (QQuickFontValueType::setPixelSize, QFont::setPointSizeF), a spacing is a QFixed.
    pub fn kept(self, field: &str, v: Value) -> Option<Value> {
        let (Vt::Font { .. }, Value::Num(n)) = (self, &v) else {
            return Some(v);
        };
        match field {
            "weight" => Some(Value::Num(n.trunc().clamp(1.0, 1000.0))),
            "pixelSize" | "pointSize" => (*n > 0.0).then_some(v),
            "letterSpacing" | "wordSpacing" => {
                // QFixed::fromReal: 26.6 fixed point, INT_MIN for what is no number.
                let fixed = if n.is_nan() {
                    i32::MIN
                } else {
                    (n * 64.0) as i32
                };
                Some(Value::Num(f64::from(fixed) / 64.0))
            }
            _ => Some(v),
        }
    }

    /// The type's `Q_PROPERTY`s in declaration order.
    pub fn fields(self) -> &'static [&'static str] {
        match self {
            Vt::Font { .. } => &FONT_FIELDS,
            Vt::Matrix4x4 { .. } => &MATRIX_FIELDS,
            Vt::Quaternion => &["scalar", "x", "y", "z"],
            Vt::Vector2D => &["x", "y"],
            Vt::Vector3D => &["x", "y", "z"],
            Vt::Vector4D => &["x", "y", "z", "w"],
            Vt::PointF => &["x", "y"],
            Vt::SizeF | Vt::Size => &["width", "height"],
            Vt::RectF | Vt::Rect => &[
                "x", "y", "width", "height", "left", "right", "top", "bottom",
            ],
        }
    }

    /// The properties that hold the value: [`Self::fields`] without the read-only ones
    /// computed from them (QRectF's `left`, `right`, `top`, `bottom`).
    pub fn data(self) -> &'static [&'static str] {
        match self {
            Vt::RectF | Vt::Rect => &["x", "y", "width", "height"],
            vt => vt.fields(),
        }
    }

    /// Whether the components are `qreal`s (QPointF, QSizeF, QRectF), not `float`s.
    pub fn real(self) -> bool {
        matches!(
            self,
            Vt::PointF | Vt::SizeF | Vt::RectF | Vt::Size | Vt::Rect
        )
    }

    /// The C++ type's name, as `toString` and conversion errors show it.
    pub fn cpp(self) -> &'static str {
        match self {
            Vt::Font { .. } => "QFont",
            Vt::Matrix4x4 { .. } => "QMatrix4x4",
            Vt::Quaternion => "QQuaternion",
            Vt::Vector2D => "QVector2D",
            Vt::Vector3D => "QVector3D",
            Vt::Vector4D => "QVector4D",
            Vt::PointF => "QPointF",
            Vt::SizeF => "QSizeF",
            Vt::RectF => "QRectF",
            Vt::Size => "QSize",
            Vt::Rect => "QRect",
        }
    }

    /// The value type a QML (`quaternion`) or C++ (`QQuaternion`) property type names.
    pub fn of_type(ty: &str) -> Option<Vt> {
        Some(match ty {
            "font" | "QFont" => Vt::FONT,
            "matrix4x4" | "QMatrix4x4" => Vt::Matrix4x4 { flags: IDENTITY },
            "quaternion" | "QQuaternion" => Vt::Quaternion,
            "vector2d" | "QVector2D" => Vt::Vector2D,
            "vector3d" | "QVector3D" => Vt::Vector3D,
            "vector4d" | "QVector4D" => Vt::Vector4D,
            "point" | "QPointF" => Vt::PointF,
            "size" | "QSizeF" => Vt::SizeF,
            "rect" | "QRectF" => Vt::RectF,
            "QSize" => Vt::Size,
            "QRect" => Vt::Rect,
            _ => return None,
        })
    }

    fn same_kind(self, o: Vt) -> bool {
        std::mem::discriminant(&self) == std::mem::discriminant(&o)
    }

    /// Whether a property of type `o` stores a value of this type as its own: the same type,
    /// or the integer one of it (a QSizeF written to a QSize property).
    pub fn stored_as(self, o: Vt) -> bool {
        self.same_kind(o) || matches!((self, o), (Vt::SizeF, Vt::Size) | (Vt::RectF, Vt::Rect))
    }

    fn proto_index(self) -> usize {
        match self {
            Vt::Font { .. } => 0,
            Vt::Matrix4x4 { .. } => 1,
            Vt::Quaternion => 2,
            Vt::Vector2D => 3,
            Vt::Vector3D => 4,
            Vt::Vector4D => 5,
            Vt::PointF => 6,
            Vt::SizeF => 7,
            Vt::RectF => 8,
            Vt::Size => 9,
            Vt::Rect => 10,
        }
    }
}

/// The prototypes of a realm's value types and Locale objects (one for all of them, as
/// QQmlValueTypeWrapper's, with `toString` alone) and of its `url` variants
/// (QV4::VariantPrototype).
#[derive(Debug, Clone)]
pub struct Protos {
    pub types: [Obj; 11],
    pub url: Obj,
    pub locale: Obj,
    /// The methods a Locale object answers with itself, by name.
    locale_methods: Obj,
    /// The prototype of a QML sequence (see [`readonly_sequence`]), whose own prototype is
    /// %Array.prototype%.
    pub sequence: Obj,
    /// XMLHttpRequest.prototype.
    pub xhr: Obj,
}

impl Protos {
    pub fn new() -> Self {
        let shared = new_obj();
        set_hidden(
            &shared,
            "toString",
            crate::eval::fn_value(crate::eval::Callable::Native("toString", to_string_method)),
        );
        let types = std::array::from_fn(|_| shared.clone());
        let url = new_obj();
        for (k, f) in [
            ("preserve", url_noop as crate::eval::NativeFn),
            ("destroy", url_noop),
            ("valueOf", url_value_of),
            ("toString", url_to_string),
        ] {
            set_hidden(
                &url,
                k,
                crate::eval::fn_value(crate::eval::Callable::Native(k, f)),
            );
        }
        let locale_methods = crate::builtins::locale_methods();
        let locale = shared;
        Protos {
            types,
            url,
            locale,
            locale_methods,
            sequence: sequence_proto(),
            xhr: crate::xhr::proto(),
        }
    }

    /// The prototype of value type or url object `b`, if it is one.
    pub fn of(&self, b: &JsObject) -> Option<&Obj> {
        match &b.internal {
            Internal::ValueType(vt) => Some(&self.types[vt.proto_index()]),
            Internal::Url(..) => Some(&self.url),
            // A QQuickAnchorLine and a QQmlScriptString are value types as any other.
            Internal::AnchorLine(_) | Internal::ScriptString(_) => Some(&self.types[0]),
            Internal::Locale(..) => Some(&self.locale),
            _ => None,
        }
    }

    /// Method `k` of Locale object `o`, bound to it: a new function object each read, as
    /// a value type's (see [`method`]).
    pub fn locale_method(&self, o: &Obj, k: &str) -> Option<Value> {
        let target = self.locale_methods.borrow().props.get(k)?.clone();
        Some(bound_to(o, target))
    }

    pub fn all(&self) -> impl Iterator<Item = &Obj> {
        let rest = [&self.url, &self.sequence];
        self.types[..1].iter().chain(rest)
    }
}

impl Default for Protos {
    fn default() -> Self {
        Self::new()
    }
}

/// `f` as the C++ `float` a math value type stores.
fn f32r(f: f64) -> f64 {
    f as f32 as f64
}

/// The value type of `v`, if it is one.
pub fn kind(v: &Value) -> Option<Vt> {
    match v {
        Value::Obj(o) => match o.borrow().internal {
            Internal::ValueType(vt) => Some(vt),
            _ => None,
        },
        _ => None,
    }
}

/// A new `vt` value with these property values (in [`Vt::fields`] order).
pub fn make(vt: Vt, vals: impl IntoIterator<Item = Value>) -> Value {
    let o = new_obj();
    {
        let b = &mut *o.borrow_mut();
        b.internal = Internal::ValueType(vt);
    }
    for (k, v) in vt.fields().iter().zip(vals) {
        crate::eval::set_own(&o, k, v);
    }
    derive(&o, vt);
    Value::Obj(o)
}

/// Set the read-only properties of value type object `o` from its [`Vt::data`].
fn derive(o: &Obj, vt: Vt) {
    if !matches!(vt, Vt::RectF | Vt::Rect) {
        return;
    }
    let [x, y, w, h] = {
        let b = o.borrow();
        let c = nums(&b, vt);
        [c[0], c[1], c[2], c[3]]
    };
    for (k, n) in [("left", x), ("right", x + w), ("top", y), ("bottom", y + h)] {
        crate::eval::set_own(o, k, Value::Num(n));
    }
}

/// A new numeric value type (`float` components rounded as the C++ type stores them).
pub fn make_num(vt: Vt, vals: &[f64]) -> Value {
    let real = vt.real();
    let int = matches!(vt, Vt::Size | Vt::Rect);
    make(
        vt,
        vals.iter().map(|&n| {
            Value::Num(if int {
                (n + 0.5).floor()
            } else if real {
                n
            } else {
                f32r(n)
            })
        }),
    )
}

/// The numeric components ([`Vt::data`]) of numeric value type object `b`.
pub(crate) fn nums(b: &JsObject, vt: Vt) -> Vec<f64> {
    vt.data()
        .iter()
        .map(|k| b.props.get(*k).map_or(0.0, Value::to_number))
        .collect()
}

/// A new `url` value (a QUrl in a QV4::VariantObject).
pub fn make_url(s: &str) -> Value {
    let o = new_obj();
    o.borrow_mut().internal = Internal::Url(Rc::from(s), None);
    Value::Obj(o)
}

/// The `url` a url property holds for the string written to it: `QUrl(s)`, which script reads as
/// `QUrl::toString()` prints it (`a%20b` is `a b`).
pub fn url_prop(s: &str) -> Value {
    let url = crate::qurl::QUrl::parse(s);
    let v = make_url(&url.to_string());
    if url.is_invalid()
        && let Value::Obj(o) = &v
    {
        o.borrow_mut().internal = Internal::Url(Rc::from(""), Some(Rc::from(s)));
    }
    v
}

/// The string a url property keeps of `url` value `v`: what the url prints, or what it was made
/// of when QUrl calls that invalid.
pub fn url_written(v: &Value) -> Option<Rc<str>> {
    match v {
        Value::Obj(o) => match &o.borrow().internal {
            Internal::Url(_, Some(written)) => Some(written.clone()),
            Internal::Url(s, None) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// QFixed::fromReal(n).toReal(): a font's letter and word spacing are kept in 64ths, and
/// what does not fit the int behind them is its lowest value.
pub fn qfixed(n: f64) -> f64 {
    let v = n * 64.0;
    let fits = v > -2147483649.0 && v < 2147483648.0;
    f64::from(if fits { v as i32 } else { i32::MIN }) / 64.0
}

/// QString::arg(float) of the math value types' `toString`.
pub(crate) fn arg(n: f64) -> String {
    crate::builtins::fmt_g6(n)
}

/// QString::number(double) / QVariant(double).toString(): the shortest round-trip form.
fn shortest(n: f64) -> String {
    crate::locale::Locale::c()
        .numeric()
        .to_string(n, b'g', crate::locale_num::SHORTEST)
}

/// What `toString` (and every string conversion) of value type object `b` gives.
pub fn display(b: &JsObject) -> Option<String> {
    let vt = match &b.internal {
        Internal::ValueType(vt) => *vt,
        Internal::Url(s, _) => return Some(s.to_string()),
        // A gadget without properties in a QV4::VariantObject: QVariant::toString() is empty,
        // so its `toString` gives the type's name and empty brackets.
        Internal::AnchorLine(_) => return Some("QQuickAnchorLine()".into()),
        Internal::ScriptString(_) => return Some("QQmlScriptString()".into()),
        _ => return None,
    };
    let s = match vt {
        Vt::Font { .. } => format!("QFont({})", font_string(b, vt)),
        // QQuickMatrix4x4ValueType has no toString: QQmlValueTypeWrapper's joins the
        // QVariant strings of its properties.
        Vt::Matrix4x4 { .. } => {
            let parts: Vec<String> = nums(b, vt).into_iter().map(shortest).collect();
            format!("QMatrix4x4({})", parts.join(", "))
        }
        _ => {
            let parts: Vec<String> = nums(b, vt).into_iter().map(arg).collect();
            format!("{}({})", vt.cpp(), parts.join(", "))
        }
    };
    Some(s)
}

/// QFont::toString of font object `b`.
fn font_string(b: &JsObject, vt: Vt) -> String {
    let Vt::Font {
        pixel_sized,
        spacing,
        raw,
    } = vt
    else {
        return String::new();
    };
    let p = |k: &str| b.props.get(k).cloned().unwrap_or_default();
    let n = |k: &str| p(k).to_number();
    let flag = |k: &str| if p(k).truthy() { "1" } else { "0" };
    let tags = |k: &str| {
        let mut out = Vec::new();
        if let Value::Obj(m) = p(k) {
            let m = m.borrow();
            let mut keys: Vec<_> = m.keys.clone();
            keys.sort();
            for t in keys {
                let v = m.props.get(&t).map_or(0.0, Value::to_number);
                out.push(format!("{t}={}", shortest(v)));
            }
        }
        let mut s = out.len().to_string();
        for t in out {
            s.push(',');
            s.push_str(&t);
        }
        s
    };
    let (pt, px) = if pixel_sized {
        ("-1".to_string(), shortest(n("pixelSize")))
    } else {
        (arg(n("pointSize")), "-1".to_string())
    };
    let mut strategy = raw.strategy;
    for (k, bit) in STRATEGY_PROPS {
        if p(k).truthy() != (k == "preferShaping") {
            strategy |= bit;
        }
    }
    let style = match (p("italic").truthy(), raw.style) {
        (false, _) => 0,
        (true, 0) => 1,
        (true, s) => s,
    };
    [
        p("family").to_js_string(),
        pt,
        px,
        raw.hint.to_string(),
        shortest(n("weight")),
        style.to_string(),
        flag("underline").into(),
        flag("strikeout").into(),
        if raw.fixed_pitch { "1" } else { "0" }.into(),
        "0".into(),
        shortest(n("capitalization")),
        if spacing { "1" } else { "0" }.into(),
        arg(n("letterSpacing")),
        arg(n("wordSpacing")),
        raw.stretch.to_string(),
        strategy.to_string(),
        p("styleName").to_js_string(),
        tags("features"),
        tags("variableAxes"),
    ]
    .join(",")
}

/// Whether value type (or url) objects `a` and `b` hold equal values
/// (QQmlValueTypeWrapper::isEqual, QV4::VariantObject::isEqualTo): `None` when neither is
/// one, so identity decides.
pub fn equal(a: &Obj, b: &Obj) -> Option<bool> {
    let (x, y) = (a.borrow(), b.borrow());
    match (&x.internal, &y.internal) {
        (Internal::Url(p, None), Internal::Url(q, None)) => Some(p == q),
        // QUrl's operator==: an invalid url is compared by what it holds.
        (Internal::Url(p, v), Internal::Url(q, w)) => {
            let held = |s: &Rc<str>, written: &Option<Rc<str>>| {
                crate::qurl::QUrl::parse(written.as_ref().unwrap_or(s))
            };
            Some(held(p, v).equals(&held(q, w)))
        }
        (Internal::ValueType(p), Internal::ValueType(q)) => Some(
            p.same_kind(*q)
                // QFont's operator== also covers what [`Vt::Font`] holds.
                && (p == q || !matches!(p, Vt::Font { .. }))
                && p.data().iter().all(|k| {
                    let (u, v) = (x.props.get(*k), y.props.get(*k));
                    match (u, v) {
                        // QPointF, QSizeF and QRectF compare their qreals fuzzily.
                        (Some(Value::Num(u)), Some(Value::Num(v))) if p.real() => {
                            fuzzy_eq_d(*u, *v)
                        }
                        (Some(Value::Obj(m)), Some(Value::Obj(n))) => {
                            let (m, n) = (m.borrow(), n.borrow());
                            m.keys.len() == n.keys.len()
                                && m.keys.iter().all(|t| {
                                    n.props.get(t).is_some_and(|w| {
                                        m.props.get(t).is_some_and(|z| z.strict_eq(w))
                                    })
                                })
                        }
                        (Some(u), Some(v)) => u.strict_eq(v),
                        _ => false,
                    }
                }),
        ),
        // A QQuickAnchorLine is the line of an item, a QQmlScriptString an expression.
        (Internal::AnchorLine(p), Internal::AnchorLine(q)) => Some(p == q),
        (Internal::ScriptString(p), Internal::ScriptString(q)) => Some(p == q),
        (x, y) if value_kind(x) || value_kind(y) => Some(false),
        _ => None,
    }
}

/// Whether an object of this kind is a value: a copy of what it was read from, equal to
/// another by what it holds (QQmlValueTypeWrapper, QV4::VariantObject).
pub fn value_kind(i: &Internal) -> bool {
    matches!(
        i,
        Internal::Url(..)
            | Internal::ValueType(_)
            | Internal::AnchorLine(_)
            | Internal::ScriptString(_)
    )
}

/// The `toString` of the value type prototypes (QQmlValueTypeWrapper::method_toString).
fn to_string_method(
    it: &mut crate::eval::Interp,
    this: &Value,
    _: &[Value],
) -> crate::eval::R<Value> {
    match this {
        Value::Obj(o)
            if matches!(
                o.borrow().internal,
                Internal::ValueType(_) | Internal::AnchorLine(_) | Internal::ScriptString(_)
            ) =>
        {
            Ok(Value::str(&display(&o.borrow()).unwrap_or_default()))
        }
        _ => it.throw("TypeError", "Type error"),
    }
}

fn url_noop(_: &mut crate::eval::Interp, _: &Value, _: &[Value]) -> crate::eval::R<Value> {
    Ok(Value::Undefined)
}

/// QV4::VariantPrototype::method_valueOf: a url's string; any other `this` itself.
fn url_value_of(_: &mut crate::eval::Interp, this: &Value, _: &[Value]) -> crate::eval::R<Value> {
    Ok(match this {
        Value::Obj(o) => match &o.borrow().internal {
            Internal::Url(s, _) => Value::Str(s.clone()),
            _ => this.clone(),
        },
        _ => this.clone(),
    })
}

/// QV4::VariantPrototype::method_toString: a url's string; undefined for any other `this`.
fn url_to_string(_: &mut crate::eval::Interp, this: &Value, _: &[Value]) -> crate::eval::R<Value> {
    Ok(match this {
        Value::Obj(o) => match &o.borrow().internal {
            Internal::Url(s, _) => Value::Str(s.clone()),
            _ => Value::Undefined,
        },
        _ => Value::Undefined,
    })
}

// ---- QtGui math, ported from qquaternion.{h,cpp}, qmatrix4x4.{h,cpp} and qvectornd.cpp ----

/// qFuzzyIsNull(float).
fn fuzzy_null(f: f32) -> bool {
    f.abs() <= 0.00001
}

/// qFuzzyIsNull(double).
fn fuzzy_null_d(d: f64) -> bool {
    d.abs() <= 0.000000000001
}

/// qFuzzyCompare(float, float).
fn fuzzy_cmp(a: f32, b: f32) -> bool {
    (a - b).abs() * 100000.0 <= a.abs().min(b.abs())
}

/// qFuzzyCompare(double, double).
fn fuzzy_cmp_d(a: f64, b: f64) -> bool {
    (a - b).abs() * 1000000000000.0 <= a.abs().min(b.abs())
}

/// QtPrivate::fuzzyCompare(float, float): qFuzzyCompare, with a zero operand compared
/// with qFuzzyIsNull.
fn fuzzy_eq(a: f32, b: f32) -> bool {
    if a == 0.0 || b == 0.0 {
        fuzzy_null(a - b)
    } else {
        fuzzy_cmp(a, b)
    }
}

/// QtPrivate::fuzzyCompare(double, double), as [`fuzzy_eq`].
fn fuzzy_eq_d(a: f64, b: f64) -> bool {
    if a == 0.0 || b == 0.0 {
        fuzzy_null_d(a - b)
    } else {
        fuzzy_cmp_d(a, b)
    }
}

/// qHypot of float components, rounded back to float.
/// qHypot over floats (QHypotHelper<float>): scaled by the largest magnitude seen so far.
fn hypot(c: &[f32]) -> f32 {
    let (mut scale, mut total) = (c[0].abs(), 1.0f32);
    for &next in &c[1..] {
        if scale.is_infinite() || (scale.is_nan() && !next.is_infinite()) {
            total = 1.0;
        } else if next.is_nan() {
            (scale, total) = (next, 1.0);
        } else if scale.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) || next.is_infinite()
        {
            (scale, total) = (next.abs(), 1.0);
        } else if next.abs() > 0.0 {
            let val = next.abs();
            if val > scale {
                let ratio = scale / next;
                (scale, total) = (val, total * ratio * ratio + 1.0);
            } else {
                let ratio = next / scale;
                total += ratio * ratio;
            }
        }
    }
    if !scale.is_finite() {
        scale
    } else if scale > 0.0 {
        scale * total.sqrt()
    } else {
        0.0
    }
}

/// A QQuaternion: (scalar, x, y, z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    pub w: f32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Quat {
    fn new(w: f32, x: f32, y: f32, z: f32) -> Self {
        Quat { w, x, y, z }
    }

    fn from_vals(v: &[f64]) -> Self {
        Quat::new(v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32)
    }

    fn vals(self) -> [f64; 4] {
        [self.w, self.x, self.y, self.z].map(f64::from)
    }

    /// operator*(QQuaternion, QQuaternion).
    fn mul(self, q2: Quat) -> Quat {
        let q1 = self;
        let yy = (q1.w - q1.y) * (q2.w + q2.z);
        let zz = (q1.w + q1.y) * (q2.w - q2.z);
        let ww = (q1.z + q1.x) * (q2.x + q2.y);
        let xx = ww + yy + zz;
        let qq = 0.5 * (xx + (q1.z - q1.x) * (q2.x - q2.y));
        let w = qq - ww + (q1.z - q1.y) * (q2.y - q2.z);
        let x = qq - xx + (q1.x + q1.w) * (q2.x + q2.w);
        let y = qq - yy + (q1.w - q1.x) * (q2.y + q2.z);
        let z = qq - zz + (q1.z + q1.y) * (q2.w - q2.x);
        Quat::new(w, x, y, z)
    }

    fn conjugated(self) -> Quat {
        Quat::new(self.w, -self.x, -self.y, -self.z)
    }

    fn length(self) -> f32 {
        hypot(&[self.x, self.y, self.z, self.w])
    }

    fn normalized(self) -> Quat {
        let len = self.length();
        if fuzzy_null(len) {
            Quat::new(0.0, 0.0, 0.0, 0.0)
        } else {
            Quat::new(self.w / len, self.x / len, self.y / len, self.z / len)
        }
    }

    /// QQuaternion::inverted: the conjugate over the squared length, computed in double.
    fn inverted(self) -> Quat {
        let [w, x, y, z] = self.vals();
        let len = w * w + x * x + y * y + z * z;
        if fuzzy_null_d(len) {
            return Quat::new(0.0, 0.0, 0.0, 0.0);
        }
        Quat::new(
            (w / len) as f32,
            (-x / len) as f32,
            (-y / len) as f32,
            (-z / len) as f32,
        )
    }

    fn dot(self, q: Quat) -> f32 {
        self.w * q.w + self.x * q.x + self.y * q.y + self.z * q.z
    }

    /// QQuaternion::rotatedVector.
    fn rotated(self, v: [f32; 3]) -> [f32; 3] {
        let r = self
            .mul(Quat::new(0.0, v[0], v[1], v[2]))
            .mul(self.conjugated());
        [r.x, r.y, r.z]
    }

    /// QQuaternion::eulerAngles (degrees: pitch about x, yaw about y, roll about z).
    fn euler(self) -> [f32; 3] {
        let len = self.length();
        let rescale = !fuzzy_null(len);
        let d = |c: f32| if rescale { c / len } else { c };
        let (xps, yps, zps, wps) = (d(self.x), d(self.y), d(self.z), d(self.w));
        let (xx, xy, xz, xw) = (xps * xps, xps * yps, xps * zps, xps * wps);
        let (yy, yz, yw) = (yps * yps, yps * zps, yps * wps);
        let (zz, zw) = (zps * zps, zps * wps);
        let sinp = -2.0 * (yz - xw);
        let (pitch, yaw, roll) = if sinp.abs() < 1.0 - 0.00001 {
            (
                sinp.asin(),
                (2.0 * (xz + yw)).atan2(1.0 - 2.0 * (xx + yy)),
                (2.0 * (xy + zw)).atan2(1.0 - 2.0 * (xx + zz)),
            )
        } else {
            (
                std::f32::consts::FRAC_PI_2.copysign(sinp),
                2.0 * yps.atan2(wps),
                0.0,
            )
        };
        [pitch, yaw, roll].map(f32::to_degrees)
    }
}

/// QMatrix4x4::Flag.
pub const IDENTITY: u8 = 0x00;
bits::bit_flags! {
const TRANSLATION: u8 = 0x01;
const SCALE: u8 = 0x02;
const ROTATION2D: u8 = 0x04;
const ROTATION: u8 = 0x08;
const PERSPECTIVE: u8 = 0x10;
}
pub const GENERAL: u8 = 0x1f;

/// A QMatrix4x4, laid out as Qt's: `m[column][row]`.
#[derive(Clone, Copy, Debug)]
pub struct Mat4 {
    pub m: [[f32; 4]; 4],
    pub flags: u8,
}

type D4 = [[f64; 4]; 4];

fn det2(m: &D4, c0: usize, c1: usize, r0: usize, r1: usize) -> f64 {
    m[c0][r0] * m[c1][r1] - m[c0][r1] * m[c1][r0]
}

#[allow(clippy::too_many_arguments)]
fn det3(m: &D4, c0: usize, c1: usize, c2: usize, r0: usize, r1: usize, r2: usize) -> f64 {
    m[c0][r0] * det2(m, c1, c2, r1, r2) - m[c1][r0] * det2(m, c0, c2, r1, r2)
        + m[c2][r0] * det2(m, c0, c1, r1, r2)
}

fn det4(m: &D4) -> f64 {
    let mut det = m[0][0] * det3(m, 1, 2, 3, 1, 2, 3);
    det -= m[1][0] * det3(m, 0, 2, 3, 1, 2, 3);
    det += m[2][0] * det3(m, 0, 1, 3, 1, 2, 3);
    det -= m[3][0] * det3(m, 0, 1, 2, 1, 2, 3);
    det
}

impl Mat4 {
    pub fn identity() -> Self {
        let mut m = [[0.0; 4]; 4];
        (0..4).for_each(|i| m[i][i] = 1.0);
        Mat4 { m, flags: IDENTITY }
    }

    /// From `m11..m44` (row-major), as QMatrix4x4(const float *values).
    pub(crate) fn from_rows(v: &[f64], flags: u8) -> Self {
        let mut m = [[0.0; 4]; 4];
        for r in 0..4 {
            for c in 0..4 {
                m[c][r] = v[r * 4 + c] as f32;
            }
        }
        Mat4 { m, flags }
    }

    fn rows(&self) -> [f64; 16] {
        std::array::from_fn(|i| f64::from(self.m[i % 4][i / 4]))
    }

    fn doubles(&self) -> D4 {
        self.m.map(|c| c.map(f64::from))
    }

    /// operator*(QMatrix4x4, QMatrix4x4).
    pub fn mul(&self, o: &Mat4) -> Mat4 {
        let flags = self.flags | o.flags;
        let (a, b) = (&self.m, &o.m);
        let mut m = [[0.0f32; 4]; 4];
        if flags < ROTATION2D {
            m[0][0] = a[0][0] * b[0][0];
            m[1][1] = a[1][1] * b[1][1];
            m[2][2] = a[2][2] * b[2][2];
            m[3][0] = a[3][0] + a[0][0] * b[3][0];
            m[3][1] = a[3][1] + a[1][1] * b[3][1];
            m[3][2] = a[3][2] + a[2][2] * b[3][2];
            m[3][3] = 1.0;
            return Mat4 { m, flags };
        }
        for c in 0..4 {
            for r in 0..4 {
                m[c][r] =
                    a[0][r] * b[c][0] + a[1][r] * b[c][1] + a[2][r] * b[c][2] + a[3][r] * b[c][3];
            }
        }
        Mat4 { m, flags }
    }

    fn zip(&self, o: &Mat4, f: impl Fn(f32, f32) -> f32) -> Mat4 {
        let m = std::array::from_fn(|c| std::array::from_fn(|r| f(self.m[c][r], o.m[c][r])));
        Mat4 { m, flags: GENERAL }
    }

    pub fn plus(&self, o: &Mat4) -> Mat4 {
        self.zip(o, |a, b| a + b)
    }

    pub fn minus(&self, o: &Mat4) -> Mat4 {
        self.zip(o, |a, b| a - b)
    }

    fn times_scalar(&self, f: f32) -> Mat4 {
        self.zip(self, |a, _| a * f)
    }

    /// operator*(QMatrix4x4, QVector4D).
    fn times_v4(&self, v: [f32; 4]) -> [f32; 4] {
        let m = &self.m;
        std::array::from_fn(|r| v[0] * m[0][r] + v[1] * m[1][r] + v[2] * m[2][r] + v[3] * m[3][r])
    }

    /// QMatrix4x4::map(QVector3D).
    pub fn map3(&self, p: [f32; 3]) -> [f32; 3] {
        let m = &self.m;
        if self.flags == IDENTITY {
            p
        } else if self.flags < ROTATION2D {
            [
                p[0] * m[0][0] + m[3][0],
                p[1] * m[1][1] + m[3][1],
                p[2] * m[2][2] + m[3][2],
            ]
        } else if self.flags < ROTATION {
            [
                p[0] * m[0][0] + p[1] * m[1][0] + m[3][0],
                p[0] * m[0][1] + p[1] * m[1][1] + m[3][1],
                p[2] * m[2][2] + m[3][2],
            ]
        } else {
            let row = |r: usize| p[0] * m[0][r] + p[1] * m[1][r] + p[2] * m[2][r] + m[3][r];
            let (x, y, z, w) = (row(0), row(1), row(2), row(3));
            if w == 1.0 {
                [x, y, z]
            } else {
                [x / w, y / w, z / w]
            }
        }
    }

    /// QMatrix4x4::map(QPointF), in qreal.
    fn map_point(&self, x: f64, y: f64) -> (f64, f64) {
        let m = self.doubles();
        if self.flags == IDENTITY {
            (x, y)
        } else if self.flags < ROTATION2D {
            (x * m[0][0] + m[3][0], y * m[1][1] + m[3][1])
        } else if self.flags < PERSPECTIVE {
            (
                x * m[0][0] + y * m[1][0] + m[3][0],
                x * m[0][1] + y * m[1][1] + m[3][1],
            )
        } else {
            let px = x * m[0][0] + y * m[1][0] + m[3][0];
            let py = x * m[0][1] + y * m[1][1] + m[3][1];
            let w = x * m[0][3] + y * m[1][3] + m[3][3];
            if w == 1.0 { (px, py) } else { (px / w, py / w) }
        }
    }

    /// QMatrix4x4::mapRect(QRectF): (x, y, width, height).
    fn map_rect(&self, r: [f64; 4]) -> [f64; 4] {
        let m = &self.m;
        let [rx, ry, rw, rh] = r;
        if self.flags < SCALE {
            return [rx + f64::from(m[3][0]), ry + f64::from(m[3][1]), rw, rh];
        }
        if self.flags < ROTATION2D {
            let mut x = rx as f32 * m[0][0] + m[3][0];
            let mut y = ry as f32 * m[1][1] + m[3][1];
            let mut w = rw as f32 * m[0][0];
            let mut h = rh as f32 * m[1][1];
            if w < 0.0 {
                w = -w;
                x -= w;
            }
            if h < 0.0 {
                h = -h;
                y -= h;
            }
            return [x, y, w, h].map(f64::from);
        }
        let pts = [
            self.map_point(rx, ry),
            self.map_point(rx + rw, ry),
            self.map_point(rx, ry + rh),
            self.map_point(rx + rw, ry + rh),
        ];
        let f = |v: f64| v as f32;
        let xmin = pts.iter().map(|p| f(p.0)).fold(f32::INFINITY, f32::min);
        let xmax = pts.iter().map(|p| f(p.0)).fold(f32::NEG_INFINITY, f32::max);
        let ymin = pts.iter().map(|p| f(p.1)).fold(f32::INFINITY, f32::min);
        let ymax = pts.iter().map(|p| f(p.1)).fold(f32::NEG_INFINITY, f32::max);
        let (xmin, xmax, ymin, ymax) = (
            f64::from(xmin),
            f64::from(xmax),
            f64::from(ymin),
            f64::from(ymax),
        );
        [xmin, ymin, xmax - xmin, ymax - ymin]
    }

    pub fn translate(&mut self, x: f32, y: f32, z: f32) {
        let f = self.flags;
        let m = &mut self.m;
        if f == IDENTITY {
            (m[3][0], m[3][1], m[3][2]) = (x, y, z);
        } else if f == TRANSLATION {
            m[3][0] += x;
            m[3][1] += y;
            m[3][2] += z;
        } else if f == SCALE {
            m[3][0] = m[0][0] * x;
            m[3][1] = m[1][1] * y;
            m[3][2] = m[2][2] * z;
        } else if f == TRANSLATION | SCALE {
            m[3][0] += m[0][0] * x;
            m[3][1] += m[1][1] * y;
            m[3][2] += m[2][2] * z;
        } else if f < ROTATION {
            m[3][0] += m[0][0] * x + m[1][0] * y;
            m[3][1] += m[0][1] * x + m[1][1] * y;
            m[3][2] += m[2][2] * z;
        } else {
            let t: [f32; 4] = std::array::from_fn(|r| m[0][r] * x + m[1][r] * y + m[2][r] * z);
            for (d, t) in m[3].iter_mut().zip(t) {
                *d += t;
            }
        }
        self.flags |= TRANSLATION;
    }

    pub fn scale(&mut self, x: f32, y: f32, z: f32) {
        let f = self.flags;
        let m = &mut self.m;
        if f < SCALE {
            (m[0][0], m[1][1], m[2][2]) = (x, y, z);
        } else if f < ROTATION2D {
            m[0][0] *= x;
            m[1][1] *= y;
            m[2][2] *= z;
        } else if f < ROTATION {
            m[0][0] *= x;
            m[0][1] *= x;
            m[1][0] *= y;
            m[1][1] *= y;
            m[2][2] *= z;
        } else {
            for (col, k) in m.iter_mut().zip([x, y, z]) {
                col.iter_mut().for_each(|v| *v *= k);
            }
        }
        self.flags |= SCALE;
    }

    /// QMatrix4x4::scale(float factor).
    fn scale_uniform(&mut self, f: f32) {
        let fl = self.flags;
        let m = &mut self.m;
        if fl < SCALE {
            (m[0][0], m[1][1], m[2][2]) = (f, f, f);
        } else if fl < ROTATION2D {
            m[0][0] *= f;
            m[1][1] *= f;
            m[2][2] *= f;
        } else if fl < ROTATION {
            m[0][0] *= f;
            m[0][1] *= f;
            m[1][0] *= f;
            m[1][1] *= f;
            m[2][2] *= f;
        } else {
            for col in &mut m[..3] {
                col.iter_mut().for_each(|v| *v *= f);
            }
        }
        self.flags |= SCALE;
    }

    /// QMatrix4x4::rotate(float angle, float x, float y, float z).
    pub(crate) fn rotate(&mut self, angle: f32, mut x: f32, mut y: f32, mut z: f32) {
        if angle == 0.0 {
            return;
        }
        let (c, mut s) = if angle == 90.0 || angle == -270.0 {
            (0.0, 1.0)
        } else if angle == -90.0 || angle == 270.0 {
            (0.0, -1.0)
        } else if angle == 180.0 || angle == -180.0 {
            (-1.0, 0.0)
        } else {
            let a = angle.to_radians();
            (a.cos(), a.sin())
        };
        // A rotation about one axis: rotate the two columns it mixes in place.
        let planar = |i: usize, j: usize, s: f32, flag: u8, this: &mut Mat4| {
            for r in 0..4 {
                let tmp = this.m[i][r];
                this.m[i][r] = tmp * c + this.m[j][r] * s;
                this.m[j][r] = this.m[j][r] * c - tmp * s;
            }
            this.flags |= flag;
        };
        if x == 0.0 {
            if y == 0.0 {
                if z != 0.0 {
                    if z < 0.0 {
                        s = -s;
                    }
                    return planar(0, 1, s, ROTATION2D, self);
                }
            } else if z == 0.0 {
                if y < 0.0 {
                    s = -s;
                }
                return planar(2, 0, s, ROTATION, self);
            }
        } else if y == 0.0 && z == 0.0 {
            if x < 0.0 {
                s = -s;
            }
            return planar(1, 2, s, ROTATION, self);
        }
        let len = f64::from(x).powi(2) + f64::from(y).powi(2) + f64::from(z).powi(2);
        if !fuzzy_cmp_d(len, 1.0) && !fuzzy_null_d(len) {
            let len = len.sqrt();
            x = (f64::from(x) / len) as f32;
            y = (f64::from(y) / len) as f32;
            z = (f64::from(z) / len) as f32;
        }
        let ic = 1.0 - c;
        let mut rot = Mat4::identity();
        let m = &mut rot.m;
        m[0][0] = x * x * ic + c;
        m[1][0] = x * y * ic - z * s;
        m[2][0] = x * z * ic + y * s;
        m[0][1] = y * x * ic + z * s;
        m[1][1] = y * y * ic + c;
        m[2][1] = y * z * ic - x * s;
        m[0][2] = x * z * ic - y * s;
        m[1][2] = y * z * ic + x * s;
        m[2][2] = z * z * ic + c;
        rot.flags = ROTATION;
        *self = self.mul(&rot);
    }

    /// QMatrix4x4::projectedRotate: a rotation projected back onto the z = 0 plane from
    /// `dist` away, in one step (QQuickRotation::applyTo uses 1024).
    #[allow(clippy::needless_range_loop)]
    pub fn projected_rotate(&mut self, angle: f32, mut x: f32, mut y: f32, mut z: f32, dist: f32) {
        if dist == 0.0 {
            return self.rotate(angle, x, y, z);
        }
        if angle == 0.0 {
            return;
        }
        let (c, mut s) = if angle == 90.0 || angle == -270.0 {
            (0.0, 1.0)
        } else if angle == -90.0 || angle == 270.0 {
            (0.0, -1.0)
        } else if angle == 180.0 || angle == -180.0 {
            (-1.0, 0.0)
        } else {
            let a = angle.to_radians();
            (a.cos(), a.sin())
        };
        let d = 1.0 / f64::from(dist);
        let m = &mut self.m;
        if x == 0.0 {
            if y == 0.0 {
                if z != 0.0 {
                    if z < 0.0 {
                        s = -s;
                    }
                    for r in 0..4 {
                        let tmp = m[0][r];
                        m[0][r] = tmp * c + m[1][r] * s;
                        m[1][r] = m[1][r] * c - tmp * s;
                    }
                    self.flags |= ROTATION2D;
                    return;
                }
            } else if z == 0.0 {
                if y < 0.0 {
                    s = -s;
                }
                let s = (f64::from(s) * d) as f32;
                for r in 0..4 {
                    m[0][r] = m[0][r] * c + m[3][r] * s;
                }
                self.flags = GENERAL;
                return;
            }
        } else if y == 0.0 && z == 0.0 {
            if x < 0.0 {
                s = -s;
            }
            let s = (f64::from(s) * d) as f32;
            for r in 0..4 {
                m[1][r] = m[1][r] * c - m[3][r] * s;
            }
            self.flags = GENERAL;
            return;
        }
        let len = f64::from(x).powi(2) + f64::from(y).powi(2) + f64::from(z).powi(2);
        if !fuzzy_cmp_d(len, 1.0) && !fuzzy_null_d(len) {
            let len = len.sqrt();
            x = (f64::from(x) / len) as f32;
            y = (f64::from(y) / len) as f32;
            z = (f64::from(z) / len) as f32;
        }
        let ic = 1.0 - c;
        let mut rot = Mat4::identity();
        let r = &mut rot.m;
        r[0][0] = x * x * ic + c;
        r[1][0] = x * y * ic - z * s;
        r[0][1] = y * x * ic + z * s;
        r[1][1] = y * y * ic + c;
        r[0][3] = (f64::from(x * z * ic - y * s) * -d) as f32;
        r[1][3] = (f64::from(y * z * ic + x * s) * -d) as f32;
        rot.flags = GENERAL;
        *self = self.mul(&rot);
    }

    /// QMatrix4x4::rotate(const QQuaternion &).
    fn rotate_q(&mut self, q: Quat) {
        let (f2x, f2y, f2z) = (q.x + q.x, q.y + q.y, q.z + q.z);
        let (f2xw, f2yw, f2zw) = (f2x * q.w, f2y * q.w, f2z * q.w);
        let (f2xx, f2xy, f2xz) = (f2x * q.x, f2x * q.y, f2x * q.z);
        let (f2yy, f2yz, f2zz) = (f2y * q.y, f2y * q.z, f2z * q.z);
        let mut rot = Mat4::identity();
        let m = &mut rot.m;
        m[0][0] = 1.0 - (f2yy + f2zz);
        m[1][0] = f2xy - f2zw;
        m[2][0] = f2xz + f2yw;
        m[0][1] = f2xy + f2zw;
        m[1][1] = 1.0 - (f2xx + f2zz);
        m[2][1] = f2yz - f2xw;
        m[0][2] = f2xz - f2yw;
        m[1][2] = f2yz + f2xw;
        m[2][2] = 1.0 - (f2xx + f2yy);
        rot.flags = ROTATION;
        *self = self.mul(&rot);
    }

    /// QMatrix4x4::lookAt.
    fn look_at(&mut self, eye: [f32; 3], center: [f32; 3], up: [f32; 3]) {
        let fwd = sub3(center, eye);
        if fwd.iter().all(|&c| fuzzy_null(c)) {
            return;
        }
        let fwd = normalize3(fwd);
        let side = normalized3(cross3(fwd, up));
        let upv = cross3(side, fwd);
        let mut m = Mat4::identity();
        for i in 0..3 {
            m.m[i][0] = side[i];
            m.m[i][1] = upv[i];
            m.m[i][2] = -fwd[i];
        }
        m.flags = ROTATION;
        *self = self.mul(&m);
        self.translate(-eye[0], -eye[1], -eye[2]);
    }

    pub fn determinant(&self) -> f64 {
        let f = self.flags;
        if f & !(TRANSLATION | ROTATION2D | ROTATION) == IDENTITY {
            return 1.0;
        }
        if f < ROTATION2D {
            return f64::from(self.m[0][0]) * f64::from(self.m[1][1]) * f64::from(self.m[2][2]);
        }
        let mm = self.doubles();
        if f < PERSPECTIVE {
            det3(&mm, 0, 1, 2, 0, 1, 2)
        } else {
            det4(&mm)
        }
    }

    pub fn inverted(&self) -> Mat4 {
        let (f, m) = (self.flags, &self.m);
        let mut inv = Mat4::identity();
        if f == IDENTITY {
            return inv;
        }
        if f == TRANSLATION {
            inv.m[3] = [-m[3][0], -m[3][1], -m[3][2], 1.0];
            inv.flags = TRANSLATION;
            return inv;
        }
        if f < ROTATION2D {
            if m[0][0] == 0.0 || m[1][1] == 0.0 || m[2][2] == 0.0 {
                return inv;
            }
            inv.m[0][0] = 1.0 / m[0][0];
            inv.m[1][1] = 1.0 / m[1][1];
            inv.m[2][2] = 1.0 / m[2][2];
            inv.m[3][0] = -m[3][0] * inv.m[0][0];
            inv.m[3][1] = -m[3][1] * inv.m[1][1];
            inv.m[3][2] = -m[3][2] * inv.m[2][2];
            inv.flags = f;
            return inv;
        }
        if f & !(TRANSLATION | ROTATION2D | ROTATION) == IDENTITY {
            // QMatrix4x4::orthonormalInverse.
            let r = &mut inv.m;
            for c in 0..3 {
                for w in 0..3 {
                    r[w][c] = m[c][w];
                }
            }
            let t: [f32; 3] = std::array::from_fn(|c| {
                -(r[0][c] * m[3][0] + r[1][c] * m[3][1] + r[2][c] * m[3][2])
            });
            r[3][..3].copy_from_slice(&t);
            inv.flags = f;
            return inv;
        }
        let mm = self.doubles();
        if f < PERSPECTIVE {
            let det = det3(&mm, 0, 1, 2, 0, 1, 2);
            if det == 0.0 {
                return inv;
            }
            let det = 1.0 / det;
            let r = &mut inv.m;
            r[0][0] = (det2(&mm, 1, 2, 1, 2) * det) as f32;
            r[0][1] = (-det2(&mm, 0, 2, 1, 2) * det) as f32;
            r[0][2] = (det2(&mm, 0, 1, 1, 2) * det) as f32;
            r[1][0] = (-det2(&mm, 1, 2, 0, 2) * det) as f32;
            r[1][1] = (det2(&mm, 0, 2, 0, 2) * det) as f32;
            r[1][2] = (-det2(&mm, 0, 1, 0, 2) * det) as f32;
            r[2][0] = (det2(&mm, 1, 2, 0, 1) * det) as f32;
            r[2][1] = (-det2(&mm, 0, 2, 0, 1) * det) as f32;
            r[2][2] = (det2(&mm, 0, 1, 0, 1) * det) as f32;
            let t: [f32; 3] =
                std::array::from_fn(|c| -r[0][c] * m[3][0] - r[1][c] * m[3][1] - r[2][c] * m[3][2]);
            r[3][..3].copy_from_slice(&t);
            inv.flags = f;
            return inv;
        }
        let det = det4(&mm);
        if det == 0.0 {
            return inv;
        }
        let det = 1.0 / det;
        let cols = [[1, 2, 3], [0, 2, 3], [0, 1, 3], [0, 1, 2]];
        for c in 0..4 {
            for r in 0..4 {
                // inv.m[c][r] = ±det3(columns other than r, rows other than c) * det.
                let [a, b, d] = cols[r];
                let [x, y, z] = cols[c];
                let sign = if (c + r) % 2 == 0 { 1.0 } else { -1.0 };
                inv.m[c][r] = (sign * det3(&mm, a, b, d, x, y, z) * det) as f32;
            }
        }
        inv.flags = f;
        inv
    }

    fn transposed(&self) -> Mat4 {
        let mut t = *self;
        for r in 0..4 {
            for c in 0..4 {
                t.m[c][r] = self.m[r][c];
            }
        }
        t.flags = if self.flags & TRANSLATION != 0 {
            GENERAL
        } else {
            self.flags
        };
        t
    }

    /// QMatrix4x4::row(index), a null vector for an index out of range.
    fn row(&self, i: i32) -> [f32; 4] {
        match usize::try_from(i) {
            Ok(i) if i < 4 => std::array::from_fn(|c| self.m[c][i]),
            _ => [0.0; 4],
        }
    }

    fn column(&self, i: i32) -> [f32; 4] {
        match usize::try_from(i) {
            Ok(i) if i < 4 => self.m[i],
            _ => [0.0; 4],
        }
    }

    /// qFuzzyCompare(QMatrix4x4, QMatrix4x4).
    fn fuzzy(&self, o: &Mat4) -> bool {
        (0..16).all(|i| fuzzy_eq(self.m[i / 4][i % 4], o.m[i / 4][i % 4]))
    }
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// QVector3D::crossProduct.
fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// QVector3D::normalized.
fn normalized3(v: [f32; 3]) -> [f32; 3] {
    let len = hypot(&v);
    if fuzzy_null(len - 1.0) {
        v
    } else if fuzzy_null(len) {
        [0.0; 3]
    } else {
        v.map(|c| c / len)
    }
}

/// QVector3D::normalize (in place: a null vector is kept).
fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len = hypot(&v);
    if fuzzy_null(len - 1.0) || fuzzy_null(len) {
        v
    } else {
        v.map(|c| c / len)
    }
}

// ---- Q_INVOKABLE methods, called as QV4::QObjectMethod calls a gadget's ----

/// A parameter type of a value type method.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum P {
    Real,
    Float,
    Int,
    Quat,
    V2,
    V3,
    V4,
    Mat,
    PointF,
    RectF,
}

impl P {
    fn cpp(self) -> &'static str {
        match self {
            P::Real => "double",
            P::Float => "float",
            P::Int => "int",
            P::Quat => "QQuaternion",
            P::V2 => "QVector2D",
            P::V3 => "QVector3D",
            P::V4 => "QVector4D",
            P::Mat => "QMatrix4x4",
            P::PointF => "QPointF",
            P::RectF => "QRectF",
        }
    }

    fn vt(self) -> Option<Vt> {
        Some(match self {
            P::Quat => Vt::Quaternion,
            P::V2 => Vt::Vector2D,
            P::V3 => Vt::Vector3D,
            P::V4 => Vt::Vector4D,
            P::Mat => Vt::Matrix4x4 { flags: IDENTITY },
            _ => return None,
        })
    }
}

type Sig = (&'static str, &'static [P]);

/// The methods of QQuickMatrix4x4ValueType, in declaration order.
const MATRIX_METHODS: &[Sig] = &[
    ("translate", &[P::V3]),
    ("rotate", &[P::Float, P::V3]),
    ("rotate", &[P::Quat]),
    ("scale", &[P::Float]),
    ("scale", &[P::Float, P::Float, P::Float]),
    ("scale", &[P::V3]),
    ("lookAt", &[P::V3, P::V3, P::V3]),
    ("times", &[P::Mat]),
    ("times", &[P::V4]),
    ("times", &[P::V3]),
    ("times", &[P::Real]),
    ("plus", &[P::Mat]),
    ("minus", &[P::Mat]),
    ("row", &[P::Int]),
    ("column", &[P::Int]),
    ("determinant", &[]),
    ("inverted", &[]),
    ("transposed", &[]),
    ("map", &[P::PointF]),
    ("mapRect", &[P::RectF]),
    ("fuzzyEquals", &[P::Mat, P::Real]),
    ("fuzzyEquals", &[P::Mat]),
];

/// The methods of QQuickQuaternionValueType, in declaration order.
const QUATERNION_METHODS: &[Sig] = &[
    ("dotProduct", &[P::Quat]),
    ("times", &[P::Quat]),
    ("times", &[P::V3]),
    ("times", &[P::Real]),
    ("plus", &[P::Quat]),
    ("minus", &[P::Quat]),
    ("normalized", &[]),
    ("inverted", &[]),
    ("conjugated", &[]),
    ("length", &[]),
    ("toEulerAngles", &[]),
    ("toVector4d", &[]),
    ("fuzzyEquals", &[P::Quat, P::Real]),
    ("fuzzyEquals", &[P::Quat]),
];

/// The methods of QQuickVector2DValueType, in declaration order.
const VECTOR2D_METHODS: &[Sig] = &[
    ("dotProduct", &[P::V2]),
    ("times", &[P::V2]),
    ("times", &[P::Real]),
    ("plus", &[P::V2]),
    ("minus", &[P::V2]),
    ("normalized", &[]),
    ("length", &[]),
    ("toVector3d", &[]),
    ("toVector4d", &[]),
    ("fuzzyEquals", &[P::V2, P::Real]),
    ("fuzzyEquals", &[P::V2]),
];

/// The methods of QQuickVector3DValueType, in declaration order.
const VECTOR3D_METHODS: &[Sig] = &[
    ("crossProduct", &[P::V3]),
    ("dotProduct", &[P::V3]),
    ("times", &[P::Mat]),
    ("times", &[P::V3]),
    ("times", &[P::Real]),
    ("plus", &[P::V3]),
    ("minus", &[P::V3]),
    ("normalized", &[]),
    ("length", &[]),
    ("toVector2d", &[]),
    ("toVector4d", &[]),
    ("fuzzyEquals", &[P::V3, P::Real]),
    ("fuzzyEquals", &[P::V3]),
];

/// The methods of QQuickVector4DValueType, in declaration order.
const VECTOR4D_METHODS: &[Sig] = &[
    ("dotProduct", &[P::V4]),
    ("times", &[P::V4]),
    ("times", &[P::Mat]),
    ("times", &[P::Real]),
    ("plus", &[P::V4]),
    ("minus", &[P::V4]),
    ("normalized", &[]),
    ("length", &[]),
    ("toVector2d", &[]),
    ("toVector3d", &[]),
    ("fuzzyEquals", &[P::V4, P::Real]),
    ("fuzzyEquals", &[P::V4]),
];

fn methods(vt: Vt) -> &'static [Sig] {
    match vt {
        Vt::Matrix4x4 { .. } => MATRIX_METHODS,
        Vt::Quaternion => QUATERNION_METHODS,
        Vt::Vector2D => VECTOR2D_METHODS,
        Vt::Vector3D => VECTOR3D_METHODS,
        Vt::Vector4D => VECTOR4D_METHODS,
        Vt::Font { .. } | Vt::PointF | Vt::SizeF | Vt::RectF | Vt::Size | Vt::Rect => &[],
    }
}

macro_rules! natives {
    ($($name:literal),* $(,)?) => {
        /// One native per method name, which dispatches on its `this`.
        const NATIVES: &[(&str, crate::eval::NativeFn)] = &[
            $(($name, |it, this, a| call(it, this, $name, a)),)*
        ];
    };
}

natives!(
    "translate",
    "rotate",
    "scale",
    "lookAt",
    "times",
    "plus",
    "minus",
    "row",
    "column",
    "determinant",
    "inverted",
    "transposed",
    "map",
    "mapRect",
    "fuzzyEquals",
    "dotProduct",
    "normalized",
    "conjugated",
    "length",
    "toEulerAngles",
    "toVector4d",
    "crossProduct",
    "toVector2d",
    "toVector3d",
);

/// Whether value type `vt` has a method `k`.
pub fn has_method(vt: Vt, k: &str) -> bool {
    methods(vt).iter().any(|s| s.0 == k)
}

/// Method `k` of value type object `o` (QQmlValueTypeWrapper::virtualGet), bound to it:
/// a new function object each read.
pub fn method(o: &Obj, vt: Vt, k: &str) -> Option<Value> {
    if k == "toString" {
        let target =
            crate::eval::fn_value(crate::eval::Callable::Native("toString", to_string_method));
        return Some(bound_to(o, target));
    }
    methods(vt).iter().find(|s| s.0 == k)?;
    let (name, f) = NATIVES.iter().find(|n| n.0 == k)?;
    let target = crate::eval::fn_value(crate::eval::Callable::Native(name, *f));
    Some(bound_to(o, target))
}

/// What script defined on value type object `o` besides the type's properties. V4 keeps
/// the members of a QQmlValueTypeWrapper beside the value: an index written to it and
/// whatever `Object.defineProperty` made. They are read after the type's properties and
/// methods, listed after them, and only an index among them is an own property to
/// `in`, `hasOwnProperty` and `Object.getOwnPropertyDescriptor`.
pub fn members(o: &Obj) -> Option<Obj> {
    let b = o.borrow();
    match b.internal {
        Internal::ValueType(_) => b.ext.as_ref()?.members.clone(),
        _ => None,
    }
}

/// Whether `o` is a value type read from a QML property. Qt makes a new wrapper at each
/// read, so what script does to the wrapper itself (not to the value) does not last.
pub fn is_property_copy(o: &Obj) -> bool {
    let b = o.borrow();
    matches!(b.internal, Internal::ValueType(_)) && b.owner.is_some()
}

/// [`members`] of value type object `o`, made on first use: as extensible as `o` is.
pub fn members_mut(o: &Obj) -> Obj {
    let mut b = o.borrow_mut();
    let fixed = b.non_extensible;
    (b.ext.get_or_insert_default().members)
        .get_or_insert_with(|| {
            let m = new_obj();
            {
                let mut mb = m.borrow_mut();
                mb.null_proto = true;
                mb.non_extensible = fixed;
            }
            m
        })
        .clone()
}

/// Whether `k` names a method object `b` answers with itself: a value type's or a
/// Locale's own, and the `toString` of either (QQmlValueTypeWrapper::virtualGetOwnProperty
/// finds them, virtualHasProperty does not).
pub fn is_own_method(b: &crate::value::JsObject, k: &str) -> bool {
    match b.internal {
        Internal::ValueType(vt) => k == "toString" || has_method(vt, k),
        Internal::Locale(..) => crate::builtins::is_locale_method(k),
        _ => false,
    }
}

/// `target` with `o` as the `this` it is always called on. V4 gives such a method no name.
pub(crate) fn bound_to(o: &Obj, target: Value) -> Value {
    let f = crate::eval::fn_value(crate::eval::Callable::Bound(Rc::new(
        crate::eval::BoundFn {
            target,
            this: Value::Obj(o.clone()),
            args: Vec::new(),
            length: 0.0,
        },
    )));
    if let Value::Obj(f) = &f {
        set_hidden(f, "name", Value::str(""));
    }
    f
}

/// QV4::SequencePrototype: `valueOf` gives the sequence as text; `length` and the four
/// methods that change it are its own, in this order.
fn sequence_proto() -> Obj {
    use crate::builtins::native;
    let p = new_obj();
    set_hidden(
        &p,
        "valueOf",
        native("valueOf", 0, |it, this, _| {
            Ok(Value::str(&it.to_string(this)?))
        }),
    );
    let refuse = |name: &'static str| -> Value {
        native(name, 0, |it, _, _| it.throw("TypeError", "Type error"))
    };
    crate::eval::define_accessor(&p, "length", true, refuse("length"), false);
    crate::eval::define_accessor(&p, "length", false, refuse("length"), false);
    for k in ["shift", "unshift", "push", "pop"] {
        set_hidden(&p, k, refuse(k));
    }
    p
}

/// Marks the array of a QML sequence that cannot be written (QV4::Sequence over a
/// read-only property, as a Locale's `weekDays` and `uiLanguages`).
struct ReadonlySequence;

/// A QML sequence of `items` that cannot be written: an array to every reader but
/// `Array.isArray`, with [`Protos::sequence`] for a prototype; a write to an element or to
/// `length` and a call that would change it throw ([`refused_sequence_write`]).
pub fn readonly_sequence(items: Vec<Value>) -> Value {
    let v = Value::array(items);
    if let Value::Obj(o) = &v {
        let mut b = o.borrow_mut();
        b.native = Some(Rc::new(ReadonlySequence));
        // Sends the writes that look at no attribute down the checked path.
        b.attrs
            .get_or_insert_default()
            .insert("length".into(), crate::value::READONLY);
    }
    v
}

pub fn is_readonly_sequence(b: &JsObject) -> bool {
    b.array.is_some()
        && b.native
            .as_ref()
            .is_some_and(|n| n.is::<ReadonlySequence>())
}

/// The TypeError Qt 6.12 throws for a write to key `k` of a read-only sequence, or for
/// the call of its method `k` that would change its length; `None` for a key that is
/// neither (a property of any other name is the object's own to keep).
pub fn refused_sequence_write(k: &str) -> Option<&'static str> {
    match k {
        "length" | "push" | "pop" | "shift" | "unshift" => Some("Type error"),
        _ if crate::eval::idx(k).is_some() => Some("Cannot insert into a readonly container"),
        _ => None,
    }
}

/// An argument converted to its parameter type.
#[derive(Clone)]
enum Arg {
    N(f64),
    Q(Quat),
    V(Vec<f32>),
    V3([f32; 3]),
    V4([f32; 4]),
    M(Mat4),
    Pt(f64, f64),
    Rect([f64; 4]),
}

impl Arg {
    fn n(&self) -> f64 {
        match self {
            Arg::N(n) => *n,
            _ => f64::NAN,
        }
    }
    fn f(&self) -> f32 {
        self.n() as f32
    }
    fn q(&self) -> Quat {
        match self {
            Arg::Q(q) => *q,
            _ => Quat::new(1.0, 0.0, 0.0, 0.0),
        }
    }
    fn v3(&self) -> [f32; 3] {
        match self {
            Arg::V3(v) => *v,
            _ => [0.0; 3],
        }
    }
    fn m(&self) -> Mat4 {
        match self {
            Arg::M(m) => *m,
            _ => Mat4::identity(),
        }
    }
    /// The components of a vector argument, as many as `n`.
    fn v(&self, n: usize) -> Vec<f32> {
        let mut c = match self {
            Arg::V(v) => v.clone(),
            Arg::V3(v) => v.to_vec(),
            Arg::V4(v) => v.to_vec(),
            _ => Vec::new(),
        };
        c.resize(n, 0.0);
        c
    }
}

/// QV4::QObjectMethod's MatchScore of `v` for a parameter of type `p` (lower is better).
fn match_score(v: &Value, p: P) -> u32 {
    let numeric = matches!(p, P::Real | P::Float | P::Int);
    match v {
        Value::Num(_) => match p {
            P::Real => 0,
            P::Float => 1,
            P::Int => 3,
            _ => 10,
        },
        Value::Bool(_) if numeric => 9,
        Value::Obj(o) => {
            let b = o.borrow();
            match (&b.internal, &b.array) {
                (Internal::ValueType(vt), _) => match p.vt() {
                    Some(t) if t.same_kind(*vt) => 0,
                    _ => 10,
                },
                (_, Some(_)) => match p {
                    P::V4 | P::Mat => 6,
                    P::V3 => 7,
                    _ => 10,
                },
                _ => 10,
            }
        }
        _ => 10,
    }
}

/// The components of a structured value `v` of type `vt` (QQmlValueTypeProvider::
/// createValueType): a value of that type, a string of them separated by commas, or an
/// object (also a value type of another kind) with its properties; an array has none.
fn structured(it: &mut crate::eval::Interp, v: &Value, vt: Vt) -> crate::eval::R<Option<Vec<f64>>> {
    let n = vt.data().len();
    let defaults = || defaults(vt);
    let o = match v {
        Value::Str(s) => {
            let parts: Vec<&str> = s.split(',').collect();
            if parts.len() != n {
                return Ok(None);
            }
            let mut out = Vec::with_capacity(n);
            for p in parts {
                match p.trim().parse::<f64>() {
                    Ok(x) => out.push(x),
                    Err(_) => return Ok(None),
                }
            }
            return Ok(Some(out));
        }
        Value::Obj(o) => o.clone(),
        _ => return Ok(None),
    };
    if let Internal::ValueType(k) = o.borrow().internal
        && k.same_kind(vt)
    {
        return Ok(Some(nums(&o.borrow(), k)));
    }
    // An array has none of the type's properties: it makes the default value.
    if o.borrow().array.is_some() {
        return Ok(Some(defaults()));
    }
    let mut out = defaults();
    let src = Value::Obj(o);
    for (i, k) in vt.data().iter().enumerate() {
        let e = it.get(&src, k)?;
        if !matches!(e, Value::Undefined) {
            out[i] = it.to_numeric(&e)?;
        }
    }
    Ok(Some(out))
}

fn incompatible<T>(it: &crate::eval::Interp) -> crate::eval::R<T> {
    it.throw(
        "TypeError",
        "Passing incompatible arguments to C++ functions from JavaScript is not allowed.",
    )
}

/// `v` as a `p` argument (QV4::CallArgument::fromValue).
fn convert(it: &mut crate::eval::Interp, v: &Value, p: P) -> crate::eval::R<Arg> {
    Ok(match p {
        P::Real | P::Float => Arg::N(it.to_numeric(v)?),
        P::Int => Arg::N(f64::from(crate::eval::to_i32(it.to_numeric(v)?))),
        P::PointF | P::RectF => {
            let Value::Obj(_) = v else {
                return incompatible(it);
            };
            let keys: &[&str] = if p == P::PointF {
                &["x", "y"]
            } else {
                &["x", "y", "width", "height"]
            };
            let mut c = [0.0; 4];
            for (i, k) in keys.iter().enumerate() {
                let e = it.get(v, k)?;
                c[i] = if matches!(e, Value::Undefined) {
                    0.0
                } else {
                    it.to_numeric(&e)?
                };
            }
            if p == P::PointF {
                Arg::Pt(c[0], c[1])
            } else {
                Arg::Rect(c)
            }
        }
        _ => {
            let vt = p.vt().unwrap_or(Vt::Vector3D);
            let flags = match v {
                Value::Obj(o) => match o.borrow().internal {
                    Internal::ValueType(Vt::Matrix4x4 { flags }) => flags,
                    _ => GENERAL,
                },
                _ => GENERAL,
            };
            // A string or object that makes no value of the type gives its default value
            // (QQmlValueTypeProvider::createValueType failing); anything else is refused.
            let c = match structured(it, v, vt)? {
                Some(c) => c,
                None if matches!(v, Value::Str(_) | Value::Obj(_)) => defaults(vt),
                None => return incompatible(it),
            };
            let c32: Vec<f32> = c.iter().map(|&x| x as f32).collect();
            match p {
                P::Quat => Arg::Q(Quat::from_vals(&c)),
                P::V2 => Arg::V(c32),
                P::V3 => Arg::V3([c32[0], c32[1], c32[2]]),
                P::V4 => Arg::V4([c32[0], c32[1], c32[2], c32[3]]),
                _ => Arg::M(Mat4::from_rows(&c, flags)),
            }
        }
    })
}

/// Call method `name` of value type `this` (QV4::QObjectMethod::callInternal: one overload
/// called precisely, or the best-matching overload as QV4::CallOverloaded picks it).
fn call(
    it: &mut crate::eval::Interp,
    this: &Value,
    name: &str,
    a: &[Value],
) -> crate::eval::R<Value> {
    let Value::Obj(o) = this else {
        return it.throw("TypeError", "Type error");
    };
    let Internal::ValueType(vt) = o.borrow().internal else {
        return it.throw("TypeError", "Type error");
    };
    let cands: Vec<&Sig> = methods(vt).iter().filter(|s| s.0 == name).collect();
    let sig = if cands.len() == 1 {
        if cands[0].1.len() > a.len() {
            return it.throw("Error", "Insufficient arguments");
        }
        cands[0]
    } else {
        let defined = a.len()
            - a.iter()
                .rev()
                .take_while(|v| matches!(v, Value::Undefined))
                .count();
        let mut best: Option<(&Sig, u32, u32, u32)> = None;
        for s in cands.iter().rev() {
            if s.1.len() > a.len() {
                continue;
            }
            let pscore = u32::from(defined != s.1.len());
            let scores: Vec<u32> = s.1.iter().zip(a).map(|(p, v)| match_score(v, *p)).collect();
            let (max, sum) = (
                scores.iter().copied().max().unwrap_or(0),
                scores.iter().sum::<u32>(),
            );
            if best.is_none_or(|(_, bp, bm, bs)| {
                pscore < bp || (pscore == bp && (max < bm || (max == bm && sum < bs)))
            }) {
                best = Some((s, pscore, max, sum));
            }
            if pscore == 0 && max == 0 {
                break;
            }
        }
        match best {
            Some((s, ..)) => s,
            None => {
                let mut msg =
                    String::from("Unable to determine callable overload.  Candidates are:");
                for s in cands.iter().rev() {
                    let ps: Vec<&str> = s.1.iter().map(|p| p.cpp()).collect();
                    msg.push_str(&format!("\n    {}({})", s.0, ps.join(",")));
                }
                return it.throw("Error", msg);
            }
        }
    };
    let mut args = Vec::with_capacity(sig.1.len());
    for (p, v) in sig.1.iter().zip(a) {
        args.push(convert(it, v, *p)?);
    }
    let vals = nums(&o.borrow(), vt);
    match vt {
        Vt::Matrix4x4 { flags } => matrix_call(it, o, Mat4::from_rows(&vals, flags), sig, &args),
        Vt::Quaternion => Ok(quaternion_call(Quat::from_vals(&vals), sig, &args)),
        _ => Ok(vector_call(
            &vals.iter().map(|&c| c as f32).collect::<Vec<_>>(),
            sig,
            &args,
        )),
    }
}

/// The vector value type of `n` components.
fn vector(c: &[f32]) -> Value {
    let vt = match c.len() {
        2 => Vt::Vector2D,
        3 => Vt::Vector3D,
        _ => Vt::Vector4D,
    };
    make_num(vt, &c.iter().map(|&x| f64::from(x)).collect::<Vec<_>>())
}

/// QVector2D/3D/4D::normalized.
fn normalized_n(v: &[f32]) -> Vec<f32> {
    let len = hypot(v);
    if fuzzy_null(len - 1.0) {
        v.to_vec()
    } else if fuzzy_null(len) {
        vec![0.0; v.len()]
    } else {
        v.iter().map(|c| c / len).collect()
    }
}

/// The methods of QQuickVector2D/3D/4DValueType on vector `v`.
fn vector_call(v: &[f32], sig: &Sig, a: &[Arg]) -> Value {
    let n = v.len();
    let zip = |o: &Arg, f: fn(f32, f32) -> f32| -> Vec<f32> {
        v.iter().zip(o.v(n)).map(|(&x, y)| f(x, y)).collect()
    };
    let resized = |k: usize| {
        let mut c = v.to_vec();
        c.resize(k, 0.0);
        vector(&c)
    };
    match (sig.0, sig.1) {
        ("dotProduct", _) => {
            let o = a[0].v(n);
            let mut d = v[0] * o[0];
            for i in 1..n {
                d += v[i] * o[i];
            }
            Value::Num(f64::from(d))
        }
        ("crossProduct", _) => {
            let o = a[0].v(3);
            vector(&[
                v[1] * o[2] - v[2] * o[1],
                v[2] * o[0] - v[0] * o[2],
                v[0] * o[1] - v[1] * o[0],
            ])
        }
        // operator*(QVector4D, QMatrix4x4): the row vector times the matrix; a 3D vector is
        // taken as (v, 1) and mapped back with QVector4D::toVector3DAffine.
        ("times", [P::Mat]) => {
            let m = a[0].m().m;
            let mut x = v.to_vec();
            x.resize(4, 1.0);
            let r: Vec<f32> = (0..4)
                .map(|c| x[0] * m[c][0] + x[1] * m[c][1] + x[2] * m[c][2] + x[3] * m[c][3])
                .collect();
            if n == 4 {
                vector(&r)
            } else if r[3] == 0.0 {
                vector(&[0.0; 3])
            } else {
                vector(&[r[0] / r[3], r[1] / r[3], r[2] / r[3]])
            }
        }
        ("times", [P::Real]) => {
            let f = a[0].f();
            vector(&v.iter().map(|c| c * f).collect::<Vec<_>>())
        }
        ("times", _) => vector(&zip(&a[0], |x, y| x * y)),
        ("plus", _) => vector(&zip(&a[0], |x, y| x + y)),
        ("minus", _) => vector(&zip(&a[0], |x, y| x - y)),
        ("normalized", _) => vector(&normalized_n(v)),
        ("length", _) => Value::Num(f64::from(hypot(v))),
        ("toVector2d", _) => resized(2),
        ("toVector3d", _) => resized(3),
        ("toVector4d", _) => resized(4),
        ("fuzzyEquals", [_, _]) => {
            let (o, eps) = (a[0].v(n), a[1].n().abs());
            Value::Bool(v.iter().zip(o).all(|(&x, y)| f64::from(x - y).abs() <= eps))
        }
        _ => Value::Bool(v.iter().zip(a[0].v(n)).all(|(&x, y)| fuzzy_eq(x, y))),
    }
}

fn v3(v: [f32; 3]) -> Value {
    make_num(Vt::Vector3D, &v.map(f64::from))
}

fn v4(v: [f32; 4]) -> Value {
    make_num(Vt::Vector4D, &v.map(f64::from))
}

fn quat(q: Quat) -> Value {
    make_num(Vt::Quaternion, &q.vals())
}

fn matrix(m: &Mat4) -> Value {
    make_num(Vt::Matrix4x4 { flags: m.flags }, &m.rows())
}

fn quaternion_call(q: Quat, sig: &Sig, a: &[Arg]) -> Value {
    let comp = |f: fn(f32, f32) -> f32, r: Quat| {
        Quat::new(f(q.w, r.w), f(q.x, r.x), f(q.y, r.y), f(q.z, r.z))
    };
    match (sig.0, sig.1) {
        ("dotProduct", _) => Value::Num(f64::from(q.dot(a[0].q()))),
        ("times", [P::Quat]) => quat(q.mul(a[0].q())),
        ("times", [P::V3]) => v3(q.rotated(a[0].v3())),
        ("times", _) => {
            let f = a[0].f();
            quat(Quat::new(q.w * f, q.x * f, q.y * f, q.z * f))
        }
        ("plus", _) => quat(comp(|x, y| x + y, a[0].q())),
        ("minus", _) => quat(comp(|x, y| x - y, a[0].q())),
        ("normalized", _) => quat(q.normalized()),
        ("inverted", _) => quat(q.inverted()),
        ("conjugated", _) => quat(q.conjugated()),
        ("length", _) => Value::Num(f64::from(q.length())),
        ("toEulerAngles", _) => v3(q.euler()),
        ("toVector4d", _) => v4([q.x, q.y, q.z, q.w]),
        ("fuzzyEquals", [_, _]) => {
            let (r, eps) = (a[0].q(), a[1].n().abs());
            let d = |x: f32, y: f32| f64::from(x - y).abs() <= eps;
            Value::Bool(d(q.w, r.w) && d(q.x, r.x) && d(q.y, r.y) && d(q.z, r.z))
        }
        _ => {
            let r = a[0].q();
            Value::Bool(
                fuzzy_eq(q.w, r.w)
                    && fuzzy_eq(q.x, r.x)
                    && fuzzy_eq(q.y, r.y)
                    && fuzzy_eq(q.z, r.z),
            )
        }
    }
}

/// Store matrix `m` into value type object `o`, notifying the property it was read from.
fn store_matrix(it: &mut crate::eval::Interp, o: &Obj, m: &Mat4) {
    let vt = Vt::Matrix4x4 { flags: m.flags };
    let same = {
        let b = &mut *o.borrow_mut();
        let same =
            matches!(b.internal, Internal::ValueType(v) if v == vt) && nums(b, vt) == m.rows();
        b.internal = Internal::ValueType(vt);
        same
    };
    for (k, v) in MATRIX_FIELDS.iter().zip(m.rows()) {
        crate::eval::set_own(o, k, Value::Num(v));
    }
    if !same {
        notify_owner(it, o);
    }
}

/// Tell the QML property value type object `o` was read from that it changed.
pub fn notify_owner(it: &mut crate::eval::Interp, o: &Obj) {
    let owner = o.borrow().owner.clone();
    if let Some((i, p)) = owner {
        it.host.sequence_changed(i, &p, &Value::Obj(o.clone()));
    }
}

fn matrix_call(
    it: &mut crate::eval::Interp,
    o: &Obj,
    mut m: Mat4,
    sig: &Sig,
    a: &[Arg],
) -> crate::eval::R<Value> {
    let mutate = |it: &mut crate::eval::Interp, m: &Mat4| {
        store_matrix(it, o, m);
        Ok(Value::Undefined)
    };
    match (sig.0, sig.1) {
        ("translate", _) => {
            let t = a[0].v3();
            m.translate(t[0], t[1], t[2]);
            mutate(it, &m)
        }
        ("rotate", [P::Quat]) => {
            m.rotate_q(a[0].q());
            mutate(it, &m)
        }
        ("rotate", _) => {
            let v = a[1].v3();
            m.rotate(a[0].f(), v[0], v[1], v[2]);
            mutate(it, &m)
        }
        ("scale", [P::Float]) => {
            m.scale_uniform(a[0].f());
            mutate(it, &m)
        }
        ("scale", [P::V3]) => {
            let v = a[0].v3();
            m.scale(v[0], v[1], v[2]);
            mutate(it, &m)
        }
        ("scale", _) => {
            m.scale(a[0].f(), a[1].f(), a[2].f());
            mutate(it, &m)
        }
        ("lookAt", _) => {
            m.look_at(a[0].v3(), a[1].v3(), a[2].v3());
            mutate(it, &m)
        }
        ("times", [P::Mat]) => Ok(matrix(&m.mul(&a[0].m()))),
        ("times", [P::V4]) => Ok(v4(match &a[0] {
            Arg::V4(v) => m.times_v4(*v),
            _ => [0.0; 4],
        })),
        ("times", [P::V3]) => Ok(v3(m.map3(a[0].v3()))),
        ("times", _) => Ok(matrix(&m.times_scalar(a[0].f()))),
        ("plus", _) => Ok(matrix(&m.plus(&a[0].m()))),
        ("minus", _) => Ok(matrix(&m.minus(&a[0].m()))),
        ("row", _) => Ok(v4(m.row(a[0].n() as i32))),
        ("column", _) => Ok(v4(m.column(a[0].n() as i32))),
        ("determinant", _) => Ok(Value::Num(m.determinant())),
        ("inverted", _) => Ok(matrix(&m.inverted())),
        ("transposed", _) => Ok(matrix(&m.transposed())),
        ("map", _) => {
            let Arg::Pt(x, y) = a[0] else {
                return Ok(Value::Undefined);
            };
            let (x, y) = m.map_point(x, y);
            Ok(make_num(Vt::PointF, &[x, y]))
        }
        ("mapRect", _) => {
            let Arg::Rect(r) = a[0] else {
                return Ok(Value::Undefined);
            };
            Ok(make_num(Vt::RectF, &m.map_rect(r)))
        }
        ("fuzzyEquals", [_, _]) => {
            let (r, eps) = (a[0].m(), a[1].n().abs());
            Ok(Value::Bool((0..16).all(|i| {
                f64::from(m.m[i / 4][i % 4] - r.m[i / 4][i % 4]).abs() <= eps
            })))
        }
        _ => Ok(Value::Bool(m.fuzzy(&a[0].m()))),
    }
}

/// The url string of `v` if it is a `url` value.
pub fn url_of(v: &Value) -> Option<String> {
    match v {
        Value::Obj(o) => match &o.borrow().internal {
            Internal::Url(s, _) => Some(s.to_string()),
            _ => None,
        },
        _ => None,
    }
}

/// `o.k = val` on value type object `o` (QQmlValueTypeWrapper::virtualPut): a property of
/// the type takes the value converted to its C++ type; any other name is not written, which
/// `false` tells.
pub fn put(
    it: &mut crate::eval::Interp,
    o: &Obj,
    vt: Vt,
    k: &str,
    val: Value,
) -> crate::eval::R<bool> {
    let k = if vt.fields().contains(&k) {
        k
    } else {
        // QQmlValueTypeWrapper::virtualPut finds a method by name and writes the property
        // that has the method's index: the one of its last overload, counted from the
        // type's own `toString` (QMatrix4x4 has none). Past the properties there is no
        // type to convert to.
        let own = usize::from(!matches!(vt, Vt::Matrix4x4 { .. }));
        let slot = match k {
            "toString" => (own == 1).then_some(0),
            _ => methods(vt).iter().rposition(|m| m.0 == k).map(|i| i + own),
        };
        match slot.map(|i| vt.fields().get(i)) {
            Some(Some(field)) => *field,
            Some(None) => {
                return it.throw("Error", format!("Cannot assign {} to ", type_name(&val)));
            }
            None => return Ok(false),
        }
    };
    // A read-only property (QRectF's edges) refuses any write, as QQmlPropertyPrivate::write
    // does after converting the value to the property's type.
    if !vt.data().contains(&k) {
        return it.throw("Error", "Cannot assign double to double");
    }
    // QQmlValueTypeWrapper::virtualPut throws this text, not an Error, for a function
    // written to a value read from a property.
    let callable = match &val {
        Value::Obj(f) => f.borrow().call.is_some(),
        Value::Native(_) => true,
        _ => false,
    };
    if callable && o.borrow().owner.is_some() {
        return Err(crate::eval::Ex::Throw(Value::str(
            "Cannot assign JavaScript function to value-type property",
        )));
    }
    let Value::Obj(before) = copy(o, vt) else {
        return Ok(true);
    };
    put_field(it, o, vt, k, val)?;
    // The owner's setter compares with the value it holds (the type's operator==) and only
    // a different one notifies.
    let unset = take_unset(&Value::Obj(o.clone()));
    if unset || equal(&before, o) != Some(true) {
        notify_owner(it, o);
    }
    Ok(true)
}

/// Hidden mark of [`unset_value`].
const UNSET: &str = "\u{1}unset";

/// Whether `v` is an [`unset_value`] nothing was written to yet; it is one no more.
pub(crate) fn take_unset(v: &Value) -> bool {
    match v {
        Value::Obj(o) => o
            .try_borrow_mut()
            .is_ok_and(|mut b| b.props.remove(UNSET).is_some()),
        _ => false,
    }
}

/// What a property of type `vt` declared without a value holds. QQmlVMEMetaObject keeps
/// QPointF, QSizeF and QRectF as themselves; any other value type is a QVariant that is
/// invalid until first written, so the first write (of a member or of the whole value) stores
/// a value that differs from it and notifies even when nothing changes.
pub fn unset_value(vt: Vt) -> Value {
    let v = default_value(vt);
    if let (Value::Obj(o), false) = (&v, matches!(vt, Vt::PointF | Vt::SizeF | Vt::RectF)) {
        set_hidden(o, UNSET, Value::Bool(true));
    }
    v
}

/// QMetaType::convert of script value `val` to the `int` or enum (`to`) a font property
/// holds: a number rounds (qRound) and wraps, a string must be an integer, `undefined` is 0.
fn font_int(it: &mut crate::eval::Interp, val: &Value, to: &str) -> crate::eval::R<i32> {
    let n = match val {
        Value::Undefined => 0.0,
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Num(n) if n.is_nan() && to == "int" => {
            return it.throw("Error", "Cannot assign double to int");
        }
        Value::Num(n) => n.round(),
        Value::Str(s) => match s.trim().parse::<i32>() {
            Ok(n) => f64::from(n),
            Err(_) => return it.throw("Error", format!("Cannot assign QString to {to}")),
        },
        v => return it.throw("Error", format!("Cannot assign {} to {to}", member_type(v))),
    };
    Ok(crate::eval::to_i32(n))
}

/// [`type_name`] of a value written to a member of a value type: an object or array stays
/// a QJSValue there.
fn member_type(v: &Value) -> String {
    match type_name(v).as_str() {
        "QVariantMap" => "QJSValue".into(),
        t => t.into(),
    }
}

/// QMetaType::convert of script value `val` to the QString a font property holds.
fn font_text(it: &mut crate::eval::Interp, val: &Value) -> crate::eval::R<Value> {
    let text = match val {
        Value::Undefined | Value::Null => String::new(),
        Value::Num(n) if n.is_nan() => "nan".into(),
        Value::Num(n) if n.is_infinite() => if *n > 0.0 { "inf" } else { "-inf" }.into(),
        Value::Obj(o) => {
            let b = o.borrow();
            match (&b.array, &b.internal) {
                // A list converts as one of QChar: of each element the first UTF-16 unit of
                // a string, a number's low 16 bits, 1 for `true`, and 0 for anything else.
                (Some(a), _) => {
                    let units: Vec<u16> = a
                        .iter()
                        .map(|e| match e {
                            Value::Str(s) => s.encode_utf16().next().unwrap_or(0),
                            Value::Str16(u) => u.first().copied().unwrap_or(0),
                            Value::Num(n) => crate::eval::to_i32(*n) as u16,
                            Value::Bool(b) => u16::from(*b),
                            _ => 0,
                        })
                        .collect();
                    return Ok(Value::from_utf16(&units));
                }
                (None, Internal::Date(_)) => {
                    drop(b);
                    it.to_string(val)?
                }
                _ => {
                    drop(b);
                    return it.throw(
                        "Error",
                        format!("Cannot assign {} to QString", member_type(val)),
                    );
                }
            }
        }
        v => it.to_string(v)?,
    };
    Ok(Value::str(&text))
}

fn put_field(
    it: &mut crate::eval::Interp,
    o: &Obj,
    vt: Vt,
    k: &str,
    val: Value,
) -> crate::eval::R<()> {
    let Vt::Font { .. } = vt else {
        let n = it.to_numeric(&val)?;
        let n = if vt.real() { n } else { f32r(n) };
        if let Vt::Matrix4x4 { .. } = vt {
            o.borrow_mut().internal = Internal::ValueType(Vt::Matrix4x4 { flags: GENERAL });
        }
        crate::eval::set_own(o, k, Value::Num(n));
        derive(o, vt);
        return Ok(());
    };
    let prim = match k {
        "family" | "styleName" => font_text(it, &val)?,
        "weight" | "pixelSize" | "capitalization" | "hintingPreference" => {
            let to = match k {
                "capitalization" => "QQuickFontEnums::Capitalization",
                "hintingPreference" => "QQuickFontEnums::HintingPreference",
                _ => "int",
            };
            Value::Num(f64::from(font_int(it, &val, to)?))
        }
        "pointSize" | "letterSpacing" | "wordSpacing" => Value::Num(it.to_numeric(&val)?),
        "features" | "variableAxes" => {
            let map = new_obj();
            if let Value::Obj(src) = &val {
                // A QVariantMap: sorted by key.
                let mut keys = src.borrow().keys.clone();
                keys.sort();
                for t in keys {
                    let e = it.get(&val, &t)?;
                    let n = it.to_numeric(&e)?;
                    if n.is_nan() || t.len() != 4 {
                        continue;
                    }
                    let n = if k == "features" {
                        f64::from(crate::eval::to_i32(n) as u32)
                    } else {
                        f32r(n)
                    };
                    crate::eval::set_own(&map, &t, Value::Num(n));
                }
            }
            Value::Obj(map)
        }
        _ => Value::Bool(val.truthy()),
    };
    font_set(o, k, prim);
    Ok(())
}

/// Stores `val`, already of the C++ type of font property `k`, as QQuickFontValueType's
/// setter does.
fn font_set(o: &Obj, k: &str, val: Value) {
    let Internal::ValueType(Vt::Font {
        mut pixel_sized,
        mut spacing,
        mut raw,
    }) = o.borrow().internal
    else {
        return;
    };
    let set = |k: &str, v: Value| crate::eval::set_own(o, k, v);
    match k {
        "bold" => {
            let w = if val.truthy() { 700.0 } else { 400.0 };
            set("weight", Value::Num(w));
            set("bold", Value::Bool(w > 500.0));
        }
        "weight" | "pixelSize" | "capitalization" | "hintingPreference" => {
            let n = crate::eval::to_i32(val.to_number());
            match k {
                "weight" => {
                    // QFont::setWeight keeps the weight within 1..=1000.
                    let n = f64::from(n.clamp(1, 1000));
                    set("weight", Value::Num(n));
                    set("bold", Value::Bool(n > 500.0));
                }
                // QFont::setPixelSize ignores a size that is not positive.
                "pixelSize" if n > 0 => {
                    let n = f64::from(n);
                    pixel_sized = true;
                    set("pixelSize", Value::Num(n));
                    set("pointSize", Value::Num(n * 72.0 / 96.0));
                }
                // QFontPrivate::capital is a 3 bit field, QFontDef::hintingPreference a
                // 2 bit one.
                "capitalization" => set(k, Value::Num(f64::from(n & 7))),
                "hintingPreference" => set(k, Value::Num(f64::from(n & 3))),
                _ => {}
            }
        }
        "pointSize" => {
            let n = val.to_number();
            // QQuickFontValueType::setPointSize: a pixel size set wins.
            if !pixel_sized && n >= 0.0 {
                set("pointSize", Value::Num(n));
                set("pixelSize", Value::Num((n * 96.0 / 72.0).trunc()));
            }
        }
        "letterSpacing" => {
            spacing = true;
            set(k, Value::Num(qfixed(val.to_number())));
        }
        "wordSpacing" => set(k, Value::Num(qfixed(val.to_number()))),
        "italic" => {
            raw.style = 0;
            set(k, val);
        }
        _ => set(k, val),
    }
    o.borrow_mut().internal = Internal::ValueType(Vt::Font {
        pixel_sized,
        spacing,
        raw,
    });
}

/// A copy of value type object `o` with property `k` set to `val` without running script
/// (a write through a property alias): `None` where QQmlPropertyPrivate::write refuses.
pub fn with_field(o: &Obj, vt: Vt, k: &str, val: &Value) -> Option<Value> {
    if !vt.data().contains(&k) {
        return None;
    }
    let Value::Obj(c) = copy(o, vt) else {
        return None;
    };
    let Vt::Font { .. } = vt else {
        let n = val.to_number();
        let n = if vt.real() { n } else { f32r(n) };
        if let Vt::Matrix4x4 { .. } = vt {
            c.borrow_mut().internal = Internal::ValueType(Vt::Matrix4x4 { flags: GENERAL });
        }
        crate::eval::set_own(&c, k, Value::Num(n));
        derive(&c, vt);
        return Some(Value::Obj(c));
    };
    let prim = match k {
        "family" | "styleName" => Value::str(&val.to_js_string()),
        "features" | "variableAxes" => return None,
        "weight" | "pixelSize" | "capitalization" | "hintingPreference"
            if matches!(val, Value::Str(_)) =>
        {
            return None;
        }
        "weight" | "pixelSize" | "capitalization" | "hintingPreference" | "pointSize"
        | "letterSpacing" | "wordSpacing" => Value::Num(val.to_number()),
        _ => Value::Bool(val.truthy()),
    };
    font_set(&c, k, prim);
    Some(Value::Obj(c))
}

/// The default value of a `vt` property: the C++ type's default constructed value.
pub fn default_value(vt: Vt) -> Value {
    match vt {
        Vt::Font { .. } => crate::qtobj::system_font_value(),
        Vt::Matrix4x4 { .. } => matrix(&Mat4::identity()),
        _ => make_num(vt, &defaults(vt)),
    }
}

/// The components of a default constructed `vt` (not a font).
fn defaults(vt: Vt) -> Vec<f64> {
    match vt {
        Vt::Matrix4x4 { .. } => Mat4::identity().rows().to_vec(),
        Vt::Quaternion => vec![1.0, 0.0, 0.0, 0.0],
        // QSizeF(): an invalid size.
        Vt::SizeF | Vt::Size => vec![-1.0, -1.0],
        _ => vec![0.0; vt.data().len()],
    }
}

/// A copy of value type object `o` (its own `features` / `variableAxes` maps copied too).
fn copy(o: &Obj, vt: Vt) -> Value {
    let b = o.borrow();
    let vals: Vec<Value> = vt
        .fields()
        .iter()
        .map(|k| match b.props.get(*k) {
            Some(Value::Obj(m)) => {
                let c = new_obj();
                let keys = m.borrow().keys.clone();
                for t in keys {
                    let e = m.borrow().props.get(&*t).cloned().unwrap_or_default();
                    crate::eval::set_own(&c, &t, e);
                }
                Value::Obj(c)
            }
            Some(v) => v.clone(),
            None => Value::Undefined,
        })
        .collect();
    make(vt, vals)
}

/// QString::toDouble: a C locale number between optional white space, `nan` or `inf`.
fn qt_double(s: &str) -> Option<f64> {
    let t = s.trim();
    match t.to_ascii_lowercase().as_str() {
        "nan" => return Some(f64::NAN),
        "inf" | "+inf" => return Some(f64::INFINITY),
        "-inf" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    t.bytes()
        .all(|c| c.is_ascii_digit() || b"+-.eE".contains(&c))
        .then(|| t.parse().ok())
        .flatten()
}

/// The components string `s` holds for a `vt` (QQmlStringConverters and the QtQuick value
/// type provider): `x,y` for a point, `WxH` for a size, `x,y,WxH` for a rect, comma
/// separated numbers for the math types.
fn from_string(s: &str, vt: Vt) -> Option<Vec<f64>> {
    let parts: Vec<&str> = match vt {
        Vt::Font { .. } => return None,
        Vt::SizeF | Vt::Size => s.split('x').collect(),
        Vt::RectF | Vt::Rect => {
            let mut c = s.splitn(3, ',');
            let (x, y, size) = (c.next()?, c.next()?, c.next()?);
            let (w, h) = size.split_once('x')?;
            vec![x, y, w, h]
        }
        _ => s.split(',').collect(),
    };
    if parts.len() != vt.data().len() {
        return None;
    }
    parts.into_iter().map(qt_double).collect()
}

/// qt_legacyToOpenTypeWeight: the OpenType weight nearest to a Qt 5 weight (0..=99).
fn legacy_weight(w: i32) -> i32 {
    const MAP: [(i32, i32); 9] = [
        (0, 100),
        (12, 200),
        (25, 300),
        (50, 400),
        (57, 500),
        (63, 600),
        (75, 700),
        (81, 800),
        (87, 900),
    ];
    let mut nearest = MAP[0];
    for m in MAP {
        if (m.0 - w).abs() < (nearest.0 - w).abs() {
            nearest = m;
        }
    }
    nearest.1
}

/// QFont::fromString over the application font: a family, optionally followed by a point
/// size, or the 10 (Qt 5) or 16 and more fields of QFont::toString.
fn font_from_string(s: &str) -> Option<Value> {
    let l: Vec<&str> = s.trim().split(',').collect();
    let count = l.len();
    if (3..10).contains(&count) || l[0].is_empty() {
        return None;
    }
    let font = default_value(Vt::FONT);
    let Value::Obj(o) = &font else {
        return None;
    };
    let set = |k: &str, v: Value| crate::eval::set_own(o, k, v);
    // QStringView::toInt and toDouble: 0 for what is not a number.
    let int = |i: usize| l[i].trim().parse::<i32>().unwrap_or(0);
    let real = |i: usize| qt_double(l[i]).unwrap_or(0.0);
    let (mut pixel_sized, mut spacing) = (false, false);
    let mut raw = FontRaw::DEFAULT;
    set("family", Value::str(l[0]));
    if count > 1 && real(1) > 0.0 {
        set("pointSize", Value::Num(real(1)));
        set("pixelSize", Value::Num((real(1) * 96.0 / 72.0).trunc()));
    }
    if count >= 10 {
        if int(2) > 0 {
            pixel_sized = true;
            set("pixelSize", Value::Num(f64::from(int(2))));
            set("pointSize", Value::Num(f64::from(int(2)) * 72.0 / 96.0));
        }
        let weight = if count >= 16 {
            int(4)
        } else {
            legacy_weight(int(4))
        };
        let weight = f64::from(weight.clamp(1, 1000));
        set("weight", Value::Num(weight));
        set("bold", Value::Bool(weight > 500.0));
        // QFontDef::style is a 2 bit field.
        let style = (int(5) & 3) as u8;
        raw.style = if style > 1 { style } else { 0 };
        raw.hint = int(3) as u8;
        raw.fixed_pitch = int(8) != 0;
        set("italic", Value::Bool(style != 0));
        set("underline", Value::Bool(int(6) != 0));
        set("strikeout", Value::Bool(int(7) != 0));
        if count >= 16 {
            set("capitalization", Value::Num(f64::from(int(10) & 7)));
            // QFont::AbsoluteSpacing is 1.
            spacing = int(11) == 1;
            set("letterSpacing", Value::Num(qfixed(real(12))));
            set("wordSpacing", Value::Num(qfixed(real(13))));
            // QFont::setStretch refuses what is out of range.
            if let Ok(stretch @ 0..=4000) = u16::try_from(int(14)) {
                raw.stretch = stretch;
            }
            let strategy = int(15) as u16;
            raw.strategy = strategy;
            for (k, bit) in STRATEGY_PROPS {
                raw.strategy &= !bit;
                set(
                    k,
                    Value::Bool((strategy & bit != 0) != (k == "preferShaping")),
                );
            }
        }
        let style = match count {
            11 => l[10],
            17.. => l[16],
            _ => "",
        };
        set("styleName", Value::str(style));
        // Then the features and the variable axes, each a count and that many `tag=value`;
        // a count that runs past the end ends the text, an entry that is no four letter
        // tag with a number is skipped.
        let mut at = 17;
        for k in ["features", "variableAxes"] {
            if at >= count {
                break;
            }
            let n = usize::try_from(int(at)).unwrap_or(0);
            at += 1;
            if at + n > count {
                break;
            }
            let mut tags: Vec<(&str, f64)> = Vec::new();
            for e in &l[at..at + n] {
                let Some((tag, v)) = e.split_once('=') else {
                    continue;
                };
                let v = if k == "features" {
                    v.trim().parse::<u32>().ok().map(f64::from)
                } else {
                    qt_double(v).map(f32r)
                };
                if let (4, Some(v)) = (tag.chars().count(), v) {
                    tags.retain(|(t, _)| *t != tag);
                    tags.push((tag, v));
                }
            }
            tags.sort_by(|a, b| a.0.cmp(b.0));
            let map = new_obj();
            for (t, v) in tags {
                crate::eval::set_own(&map, t, Value::Num(v));
            }
            set(k, Value::Obj(map));
            at += n;
        }
    }
    o.borrow_mut().internal = Internal::ValueType(Vt::Font {
        pixel_sized,
        spacing,
        raw,
    });
    Some(font)
}

/// The font a script object makes when assigned to a `font` property: the application
/// font with each font property the object has written to it; one that does not convert
/// is skipped.
fn font_from_object(it: &mut crate::eval::Interp, src: &Value) -> crate::eval::R<Value> {
    let font = default_value(Vt::FONT);
    let (Value::Obj(o), Value::Obj(s)) = (&font, src) else {
        return Ok(font);
    };
    for k in FONT_FIELDS {
        if !s.borrow().props.contains_key(k) {
            continue;
        }
        let v = it.get(src, k)?;
        let vt = match o.borrow().internal {
            Internal::ValueType(vt) => vt,
            _ => Vt::FONT,
        };
        let _ = put_field(it, o, vt, k, v);
    }
    Ok(font)
}

/// `v` stored into a `vt` property without running script (a binding's result): a copy of
/// a value of the type, or the value its components (string, own properties of an object
/// or of a value type of another kind) make; `null`, an array, and a string that does not
/// hold a math type give the default value.
pub fn convert_plain(v: &Value, vt: Vt) -> Option<Value> {
    let font = matches!(vt, Vt::Font { .. });
    let Value::Obj(o) = v else {
        return match v {
            Value::Null => Some(default_value(vt)),
            Value::Str(_) | Value::Str16(_) if font => font_from_string(&v.to_js_string()),
            Value::Str(_) | Value::Str16(_) => match from_string(&v.to_js_string(), vt) {
                Some(c) => Some(from_components(vt, &c)),
                // QQmlStringConverters refuses; the value type provider's default stays.
                None if vt.real() => None,
                None => Some(default_value(vt)),
            },
            _ => None,
        };
    };
    let k = match o.borrow().internal {
        Internal::ValueType(k) => Some(k),
        _ => None,
    };
    if let Some(k) = k
        && k.same_kind(vt)
    {
        return Some(copy(o, k));
    }
    if font {
        // Another value type or an array is no font: the default one is stored.
        return (k.is_some() || o.borrow().array.is_some()).then(|| default_value(vt));
    }
    if matches!(k, Some(Vt::Font { .. })) {
        return None;
    }
    let b = o.borrow();
    let mut c = defaults(vt);
    if b.array.is_some() {
        return Some(from_components(vt, &c));
    }
    for (i, k) in vt.data().iter().enumerate() {
        if let Some(e) = b.props.get(*k) {
            c[i] = e.to_number();
        }
    }
    Some(from_components(vt, &c))
}

fn from_components(vt: Vt, c: &[f64]) -> Value {
    let vt = match vt {
        Vt::Matrix4x4 { .. } => Vt::Matrix4x4 { flags: GENERAL },
        vt => vt,
    };
    make_num(vt, c)
}

/// QML's name of the type of `v` in a "Cannot assign" error.
pub(crate) fn type_name(v: &Value) -> String {
    match v {
        Value::Undefined => "[undefined]".into(),
        Value::Null => "std::nullptr_t".into(),
        Value::Bool(_) => "bool".into(),
        Value::Num(n) if n.fract() == 0.0 && n.abs() <= f64::from(i32::MAX) => "int".into(),
        Value::Num(_) => "double".into(),
        Value::Str(_) => "QString".into(),
        Value::Obj(o) if o.borrow().call.is_some() => "JavaScript function".into(),
        Value::Native(_) => "JavaScript function".into(),
        Value::Obj(o) => match &o.borrow().internal {
            Internal::ValueType(vt) => vt.cpp().into(),
            Internal::Url(..) => "QUrl".into(),
            Internal::AnchorLine(_) => "QQuickAnchorLine".into(),
            Internal::ScriptString(_) => "QQmlScriptString".into(),
            Internal::Date(_) => "QDateTime".into(),
            Internal::RegExp { .. } => "QRegularExpression".into(),
            Internal::ArrayBuffer(..) => "QByteArray".into(),
            _ if o.borrow().array.is_some() => "QJSValue".into(),
            _ => "QVariantMap".into(),
        },
        Value::Str16(_) => "QString".into(),
        Value::Color(_) => "QColor".into(),
        Value::Item(_) => "QObject*".into(),
        _ => "QJSValue".into(),
    }
}

/// An argument `v` of a call to a signal declared in QML, as the parameter of QML type `ty`
/// receives it (QV4::CallArgument::fromValue). Nothing is refused: a number or a string is
/// what JavaScript converts `v` to, and a value with no conversion arrives as the type's
/// default (an invalid colour or date, an empty url or list, `null` for an object type).
pub fn signal_arg(it: &mut crate::eval::Interp, ty: &str, v: Value) -> crate::eval::R<Value> {
    let failed = |v: &Value, to: &str| {
        tracing::warn!(
            target: "ri::qml",
            "Could not convert argument from {} to {to}: passing incompatible arguments to signals is not supported",
            v.to_js_string()
        );
    };
    if let Some(vt) = Vt::of_type(ty) {
        if matches!(&v, Value::Obj(o) if o.borrow().array.is_none())
            && !matches!(vt, Vt::Font { .. })
            && let Some(c) = structured(it, &v, vt)?
        {
            return Ok(from_components(vt, &c));
        }
        return Ok(match convert_plain(&v, vt) {
            Some(c) if matches!(v, Value::Obj(_)) => c,
            _ => {
                failed(&v, vt.cpp());
                default_value(vt)
            }
        });
    }
    if let Some(elem) = ty.strip_prefix("list<").and_then(|t| t.strip_suffix('>')) {
        let items = match &v {
            Value::Obj(o) => o.borrow().array.as_ref().map(|a| a.to_vec()),
            _ => None,
        };
        let Some(items) = items else {
            failed(&v, &format!("list<{elem}>"));
            return Ok(Value::array(Vec::new()));
        };
        let mut out = Vec::with_capacity(items.len());
        for e in items {
            out.push(signal_arg(it, elem, e)?);
        }
        return Ok(Value::array(out));
    }
    Ok(match ty {
        "int" => Value::Num(f64::from(crate::eval::to_i32(it.to_numeric(&v)?))),
        "real" | "double" | "number" => Value::Num(it.to_numeric(&v)?),
        "bool" => Value::Bool(v.truthy()),
        "string" => match v {
            Value::Str(_) | Value::Str16(_) => v,
            v => Value::str(&it.to_string(&v)?),
        },
        "color" => match &v {
            Value::Color(_) => v,
            Value::Str(s) => Value::Color(crate::builtins::parse_color(s)),
            Value::Null | Value::Num(_) => Value::Color(crate::color::Color::INVALID),
            _ => {
                failed(&v, "QColor");
                Value::Color(crate::color::Color::INVALID)
            }
        },
        "url" => match &v {
            Value::Str(s) => url_prop(s),
            _ if url_of(&v).is_some() => v,
            _ => {
                failed(&v, "QUrl");
                make_url("")
            }
        },
        "date" => match &v {
            Value::Obj(o) if matches!(o.borrow().internal, Internal::Date(_)) => v,
            Value::Str(s) => crate::builtins::new_date(it, crate::builtins::parse_date_local(s)),
            _ => {
                failed(&v, "QDateTime");
                crate::builtins::new_date(it, f64::NAN)
            }
        },
        "var" | "variant" => v,
        // An object type: an object of that type, else null.
        t if t.starts_with(|c: char| c.is_ascii_uppercase()) => match &v {
            Value::Item(i) if it.host.item_is(*i, t) => v,
            Value::Null | Value::Undefined => Value::Null,
            _ => {
                failed(&v, t);
                Value::Null
            }
        },
        _ => v,
    })
}

/// The properties of the `color` value type, in QQuickColorValueType's order: the ten
/// script can write, then the read-only two.
pub const COLOR_PROPERTIES: [&str; 12] = [
    "r",
    "g",
    "b",
    "a",
    "hsvHue",
    "hsvSaturation",
    "hsvValue",
    "hslHue",
    "hslSaturation",
    "hslLightness",
    "valid",
    "rgba",
];

/// Whether `v` is a value of a value type or a Locale: two of them are the same value when
/// their fields are equal (QQmlValueTypeWrapper::virtualIsEqualTo), also as Map and Set keys.
pub fn is_value(v: &Value) -> bool {
    match v {
        Value::Color(_) => true,
        Value::Obj(o) => matches!(
            o.borrow().internal,
            Internal::ValueType(_) | Internal::Locale(..)
        ),
        _ => false,
    }
}

/// Whether `k` is a property of a color.
pub fn is_color_property(k: &str) -> bool {
    COLOR_PROPERTIES.contains(&k)
}

/// Whether a color answers `k` with itself: a property or a method of the type
/// (QQmlValueTypeWrapper::virtualGetOwnProperty finds both).
pub fn is_color_own(k: &str) -> bool {
    is_color_property(k) || matches!(k, "toString" | "lighter" | "darker" | "alpha" | "tint")
}

/// The color the properties of object `src` make (QQmlValueTypeProvider's doWriteProperties):
/// an invalid color with each property of the type that `src` has, own or inherited and not
/// `undefined`, written in the type's order.
fn color_from_object(it: &mut crate::eval::Interp, src: &Value) -> crate::eval::R<Value> {
    let mut c = crate::color::Color::INVALID;
    for k in &COLOR_PROPERTIES[..10] {
        let v = it.get(src, k)?;
        if !matches!(v, Value::Undefined) {
            c = color_put(it, c, k, &v)?;
        }
    }
    Ok(Value::Color(c))
}

/// Whether `k` is a property of the `color` value type (QQuickColorValueType).
pub fn is_color_field(k: &str) -> bool {
    matches!(
        k,
        "r" | "g"
            | "b"
            | "a"
            | "hsvHue"
            | "hsvSaturation"
            | "hsvValue"
            | "hslHue"
            | "hslSaturation"
            | "hslLightness"
            | "valid"
    )
}

/// `c.k = v` for color `c` (QQuickColorValueType's setters over QColor's): the color with
/// that component changed, which the caller stores where it read `c`. An alpha outside 0..1
/// is clamped; a red, green or blue outside it makes an extended RGB color; an HSV or HSL
/// component outside its range leaves an invalid color; `valid` is read-only.
pub fn color_put(
    it: &mut crate::eval::Interp,
    c: crate::color::Color,
    k: &str,
    v: &Value,
) -> crate::eval::R<crate::color::Color> {
    if k == "valid" {
        return it.throw("Error", format!("Cannot assign {} to bool", type_name(v)));
    }
    Ok(color_with(c, k, it.to_numeric(v)?))
}

/// `c` with its property `k` (one of [`is_color_field`] but `valid`) set to `x`.
pub fn color_with(c: crate::color::Color, k: &str, x: f64) -> crate::color::Color {
    use crate::color::Color;
    let x32 = x as f32;
    match k {
        "a" => c.with_alpha_f(x32),
        "r" | "g" | "b" => c.with_rgb_f(usize::from(k == "g") + 2 * usize::from(k == "b"), x32),
        "hsvHue" | "hsvSaturation" | "hsvValue" => {
            let (mut h, mut s, mut val) = (c.hsv_hue_f(), c.hsv_saturation_f(), c.value_f());
            match k {
                "hsvHue" => h = x32,
                "hsvSaturation" => s = x32,
                _ => val = x32,
            }
            Color::from_hsv_f(h, s, val, c.alpha_f())
        }
        _ => {
            let (mut h, mut s, mut l) = (c.hsl_hue_f(), c.hsl_saturation_f(), c.lightness_f());
            match k {
                "hslHue" => h = x32,
                "hslSaturation" => s = x32,
                _ => l = x32,
            }
            Color::from_hsl_f(h, s, l, c.alpha_f())
        }
    }
}

/// What a property is to QQmlPropertyPrivate::write ([`qqml_property_write`]).
pub enum WriteTarget<'a> {
    /// A property of the QML or C++ type of this name (`int`, `QString`, `point`, `var`).
    Type(&'a str),
    /// A `list<T>` of that element type.
    List(&'a str),
    /// An enum property: the value of the enum's key of a name, and whether the enum's C++
    /// type is signed (one of its values is negative).
    Enum(&'a dyn Fn(&str) -> Option<f64>, bool),
    /// A property that points to an object: whether an object is of its type.
    Object(&'a dyn Fn(usize) -> bool),
}

/// Marks the object a QVariantMap reads as: the copy of a script object that went through
/// a QVariant (QV4::VariantAssociationObject, a `V4ReferenceObject` to
/// `Object.prototype.toString`).
struct VariantMap;

pub fn is_variant_map(b: &JsObject) -> bool {
    b.native.as_ref().is_some_and(|n| n.is::<VariantMap>())
}

/// Whether `v` is an object script made with nothing behind it, which a QVariant holds as
/// a QVariantMap.
pub fn is_script_map(v: &Value) -> bool {
    match v {
        Value::Obj(o) => {
            let b = o.borrow();
            matches!(b.internal, Internal::Ordinary) && b.array.is_none() && b.call.is_none()
        }
        _ => false,
    }
}

/// `v` as the QVariant a QVariantMap of initial properties holds it as, read back by
/// script: an object and an array are copies (a QVariantMap, a QVariantList), a Date is
/// another Date, a symbol is its description's string.
pub fn through_variant(v: &Value) -> Value {
    match v {
        Value::Symbol(_) => Value::str(&v.to_js_string()),
        Value::Obj(o) => {
            let b = o.borrow();
            if let Some(a) = &b.array {
                return Value::array(a.iter().map(through_variant).collect());
            }
            match b.internal {
                Internal::Date(ms) => crate::builtins::plain_date(ms),
                Internal::ValueType(vt) => copy(o, vt),
                Internal::Ordinary if b.call.is_none() => {
                    let m = crate::eval::new_obj();
                    for k in b.keys.iter() {
                        if let Some(e) = b.props.get(k) {
                            crate::eval::set_own(&m, k, through_variant(e));
                        }
                    }
                    m.borrow_mut().native = Some(Rc::new(VariantMap));
                    Value::Obj(m)
                }
                _ => v.clone(),
            }
        }
        v => v.clone(),
    }
}

/// QJSValue::toVariant of `v`, read back by script: what a view's `model` holds
/// (QQuickItemView::setModel, QQuickRepeater::setModel). An object script made, a String
/// or Number object and an Error are a QVariantMap of their own properties, a symbol is
/// its description's string.
pub fn js_value_variant(v: &Value) -> Value {
    let map = match v {
        Value::Symbol(_) => return through_variant(v),
        Value::Obj(o) => {
            let b = o.borrow();
            b.call.is_none()
                && b.native.is_none()
                && match &b.internal {
                    Internal::Ordinary => b.array.is_none(),
                    Internal::Primitive(p) => !matches!(p, Value::Bool(_)),
                    Internal::Error => true,
                    _ => false,
                }
        }
        _ => false,
    };
    let Value::Obj(o) = v else { return v.clone() };
    if !map {
        return v.clone();
    }
    let b = o.borrow();
    let m = crate::eval::new_obj();
    for k in b.keys.iter() {
        if let Some(e) = b.props.get(k) {
            crate::eval::set_own(&m, k, through_variant(e));
        }
    }
    m.borrow_mut().native = Some(Rc::new(VariantMap));
    Value::Obj(m)
}

/// QVariant::toDouble of `v` for a number property: a number, a bool, or a string
/// QString::toDouble reads.
fn variant_double(v: &Value) -> Option<f64> {
    match v {
        Value::Num(n) => Some(*n),
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        Value::Str(s) => qt_double(s),
        _ => None,
    }
}

/// A number stored in an `int`: qRound, then the truncation of the cast.
fn variant_int(v: &Value) -> Option<f64> {
    let x = variant_double(v)?;
    (!x.is_nan()).then(|| f64::from((x.round() as i64) as i32))
}

fn variant_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Num(n) => Some(!n.is_nan() && n.round() != 0.0),
        Value::Str(s) => Some(!(s.is_empty() || &**s == "0" || s.eq_ignore_ascii_case("false"))),
        Value::Str16(_) => Some(true),
        _ => None,
    }
}

/// QVariant::toString of `v`, `None` for a type that has no conversion to QString.
fn variant_string(v: &Value) -> Option<Value> {
    Some(match v {
        Value::Null => Value::str(""),
        Value::Bool(_) | Value::Symbol(_) => Value::str(&v.to_js_string()),
        Value::Num(n) if *n == 0.0 => Value::str("0"),
        Value::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => Value::str(&v.to_js_string()),
        Value::Num(n) => Value::str(&shortest(*n)),
        Value::Str(_) | Value::Str16(_) => v.clone(),
        Value::Color(c) => Value::str(&crate::builtins::color_name(c)),
        Value::Obj(o) => {
            let b = o.borrow();
            // A list of numbers converts as a QByteArray would: each is a character.
            if let Some(a) = &b.array {
                let units: Option<Vec<u16>> = a
                    .iter()
                    .map(|e| match e {
                        Value::Num(n) if n.fract() == 0.0 => Some(*n as u16),
                        _ => None,
                    })
                    .collect();
                return Some(Value::from_utf16(&units.unwrap_or_default()));
            }
            match &b.internal {
                Internal::Date(ms) if ms.is_nan() => Value::str(""),
                Internal::Date(ms) => {
                    Value::str(&crate::builtins::qt_format(*ms, "yyyy-MM-ddTHH:mm:ss.zzz"))
                }
                Internal::Url(s, _) => Value::str(s),
                Internal::ValueType(vt @ Vt::Font { .. }) => Value::str(&font_string(&b, *vt)),
                _ => return None,
            }
        }
        _ => return None,
    })
}

/// The type of property `field` of a value type (`None`: of a `color`) as
/// [`qqml_property_write`] names it, `None` for a name the type has no property of.
pub fn field_type(vt: Option<Vt>, field: &str) -> Option<&'static str> {
    let Some(vt) = vt else {
        return (COLOR_PROPERTIES.contains(&field) && field != "valid").then_some("real");
    };
    if !vt.fields().contains(&field) {
        return None;
    }
    Some(match (vt, field) {
        (Vt::Font { .. }, "family" | "styleName") => "string",
        (Vt::Font { .. }, "pointSize" | "letterSpacing" | "wordSpacing") => "real",
        (Vt::Font { .. }, "weight" | "pixelSize" | "capitalization" | "hintingPreference") => "int",
        (Vt::Font { .. }, "features" | "variableAxes") => "var",
        (Vt::Font { .. }, _) => "bool",
        _ => "real",
    })
}

/// One value of a scalar type `ty` for [`qqml_property_write`], without running script.
fn variant_scalar(ty: &str, v: &Value) -> Option<Value> {
    match ty {
        "int" => variant_int(v).map(Value::Num),
        "real" | "double" | "qreal" | "float" | "number" => variant_double(v).map(Value::Num),
        "bool" => variant_bool(v).map(Value::Bool),
        "string" | "QString" => variant_string(v),
        "url" | "QUrl" => match v {
            Value::Str(s) => Some(url_written(&url_prop(s)).map_or(v.clone(), Value::Str)),
            _ => url_written(v).map(Value::Str),
        },
        "date" | "QDateTime" => match v {
            Value::Str(s) => crate::builtins::qt_iso_datetime(s).map(crate::builtins::plain_date),
            _ => crate::builtins::date_ms(v).map(|ms| crate::builtins::plain_date(ms.trunc())),
        },
        _ => None,
    }
}

/// A `list<elem>` made of `v` (QQmlPropertyPrivate::write, the sequence branch): each
/// element of a list converted; any other value makes a list of one.
fn variant_list(elem: &str, v: &Value) -> Value {
    let zero = match elem {
        "int" | "real" | "double" => Value::Num(0.0),
        "bool" => Value::Bool(false),
        "var" | "variant" => Value::Undefined,
        _ => Value::str(""),
    };
    let var = matches!(elem, "var" | "variant");
    let one = |e: &Value| match var {
        true => Some(through_variant(e)),
        false => variant_scalar(elem, e),
    };
    let list = match v {
        Value::Obj(o) => o.borrow().array.clone(),
        _ => None,
    };
    let Some(list) = list else {
        return Value::array(match (elem, v) {
            // A string is the list of its characters.
            ("int", Value::Str(s)) => s.encode_utf16().map(|u| Value::Num(u.into())).collect(),
            ("string", Value::Str(s)) => s
                .chars()
                .map(|c| Value::str(c.encode_utf8(&mut [0; 4])))
                .collect(),
            _ => vec![one(v).unwrap_or(zero)],
        });
    };
    // A QList<QUrl> takes a list of urls only as a whole, and script's is one of QVariants.
    if matches!(elem, "url" | "QUrl") {
        return Value::array(vec![zero]);
    }
    // QMetaType::convert writes into the element before it: a string that is not of the
    // type leaves the type's default there, a value of a type nothing converts leaves what
    // the element before it left.
    let mut out: Vec<Value> = Vec::with_capacity(list.len());
    for e in &list {
        let kept = match e {
            Value::Str(_) => None,
            _ => out.last().cloned(),
        };
        out.push(one(e).or(kept).unwrap_or_else(|| zero.clone()));
    }
    Value::array(out)
}

/// `val` as QQmlPropertyPrivate::write stores it in a property (qtdeclarative v6.12.0,
/// src/qml/qml/qqmlproperty.cpp: QQmlPropertyPrivate::write, writeEnumProperty,
/// tryConvertAndAssign and convertToWriteTargetType): the rule of a write from C++, which
/// the initial properties of Component.createObject follow. `None` where it refuses.
/// Script's own write is another rule, [`qobject_wrapper_set_property`].
pub fn qqml_property_write(
    it: &mut crate::eval::Interp,
    target: &WriteTarget,
    val: &Value,
) -> crate::eval::R<Option<Value>> {
    let ty = match target {
        WriteTarget::Type(ty) => *ty,
        WriteTarget::List(elem) => return Ok(Some(variant_list(elem, val))),
        WriteTarget::Enum(key, signed) => {
            // The number goes in as an int and the property reads it back as the enum's own
            // type, which is unsigned unless the enum has a negative value: -1 is 4294967295.
            return Ok(match val {
                Value::Str(s) => key(s).map(Value::Num),
                Value::Num(_) | Value::Bool(_) => variant_int(val)
                    .map(|n| {
                        if *signed {
                            n
                        } else {
                            f64::from(n as i32 as u32)
                        }
                    })
                    .map(Value::Num),
                _ => None,
            });
        }
        WriteTarget::Object(is) => {
            return Ok(match val {
                Value::Null => Some(Value::Null),
                Value::Item(id) if is(*id) => Some(val.clone()),
                _ => None,
            });
        }
    };
    // What a QVariant holds as a QVariantMap, a QVariantList, a QRegularExpression.
    let (map, list, regexp, date, function) = match val {
        Value::Obj(o) => {
            let b = o.borrow();
            (
                matches!(b.internal, Internal::Ordinary) && b.array.is_none() && b.call.is_none(),
                b.array.is_some(),
                matches!(b.internal, Internal::RegExp { .. }),
                matches!(b.internal, Internal::Date(_)),
                b.call.is_some(),
            )
        }
        Value::Native(_) => (false, false, false, false, true),
        _ => (false, false, false, false, false),
    };
    if let Some(vt) = Vt::of_type(ty) {
        let font = matches!(vt, Vt::Font { .. });
        return Ok(match val {
            Value::Str(s) if font => font_from_string(s),
            Value::Str(s) => match from_string(s, vt) {
                Some(c) => Some(from_components(vt, &c)),
                // QQmlStringConverters makes these three; a vector's own constructor
                // takes any string.
                None if matches!(vt, Vt::PointF | Vt::SizeF | Vt::RectF) => None,
                None => Some(default_value(vt)),
            },
            Value::Obj(o) if kind(val).is_some_and(|k| k.same_kind(vt)) => Some(copy(o, vt)),
            Value::Obj(_) if list || regexp || date || url_of(val).is_some() => None,
            Value::Obj(_) if map && font => Some(font_from_object(it, val)?),
            Value::Obj(_) if map => structured(it, val, vt)?.map(|c| from_components(vt, &c)),
            // An object's properties of the type's names make the value.
            Value::Item(_) if !font => {
                let mut c = defaults(vt);
                for (i, k) in vt.data().iter().enumerate() {
                    let e = it.get(val, k)?;
                    if !matches!(e, Value::Undefined) {
                        c[i] = it.to_numeric(&e)?;
                    }
                }
                Some(from_components(vt, &c))
            }
            Value::Null | Value::Item(_) | Value::Color(_) | Value::Native(_) => {
                Some(default_value(vt))
            }
            Value::Obj(_) => Some(convert_plain(val, vt).unwrap_or_else(|| default_value(vt))),
            _ => None,
        });
    }
    Ok(match ty {
        "var" | "variant" | "QVariant" | "QJSValue" => Some(through_variant(val)),
        // QQuickColorValueType has a constructor from a string, so what converts to one
        // makes the color it names or an invalid one; an object makes one of its properties.
        "color" | "QColor" => match val {
            Value::Color(_) => Some(val.clone()),
            Value::Undefined => None,
            _ if list || regexp => None,
            _ if map => Some(color_from_object(it, val)?),
            _ => Some(Value::Color(match variant_string(val) {
                Some(s) => crate::builtins::parse_color(s.to_js_string().trim()),
                None => crate::color::Color::INVALID,
            })),
        },
        _ if function => None,
        ty => variant_scalar(ty, val),
    })
}

/// A type a property that is no object, list or value type holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scalar {
    Int,
    Real,
    Bool,
    String,
    Url,
    Color,
    Date,
}

/// What a property holds, for a value written to it.
pub enum SlotKind {
    /// A `var`, which holds any value.
    Var,
    /// A QVariant property of a Qt type: any value but a function.
    Variant,
    Scalar(Scalar),
    Value(Vt),
    /// A `list<T>` of values of that QML type.
    List(&'static str),
    /// A QQmlListProperty: a `list<T>` a QML document declares, of objects of that QML
    /// type, or one of a Qt type, of objects of that C++ class.
    Objects(String),
    /// An enum: whether it has no negative value (the compiler then stores it unsigned), and
    /// the value of each of its keys.
    Enum {
        unsigned: bool,
        keys: Vec<(String, f64)>,
    },
    /// An object of the QML type, or the C++ class, of that name.
    Object(String),
    /// A QQuickAnchorLine: a script's write takes a line alone.
    AnchorLine,
    /// A QQmlScriptString, which no script makes: a script's write takes a primitive for
    /// the expression and refuses an object.
    ScriptString,
    /// A C++ type whose rule nobody compared with Qt: it takes the value as script has it.
    Unknown,
}

/// Which of Qt's writers a property has.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotRule {
    /// QV4::QObjectWrapper::setProperty for a script, QQmlBinding for a binding.
    Property,
    /// A BINDABLE property, whose binding is a QQmlPropertyBinding: it converts as script
    /// does (`Number(v)`, `String(v)`).
    Bindable,
    /// A property of a value type property (`font.pixelSize`): QQmlValueTypeWrapper writes
    /// it as C++ does ([`qqml_property_write`]).
    Field,
}

/// A property as a script's write and a binding's result see it.
pub struct Slot {
    pub kind: SlotKind,
    /// The type as Qt's messages name it (`QQuickItem*`, `QList<int>`).
    pub to: String,
    pub rule: SlotRule,
}

impl Slot {
    fn scalar(ty: &str) -> Option<(Scalar, &'static str)> {
        Some(match ty {
            "int" => (Scalar::Int, "int"),
            "real" | "double" | "qreal" => (Scalar::Real, "double"),
            "bool" => (Scalar::Bool, "bool"),
            "string" | "QString" => (Scalar::String, "QString"),
            "url" | "QUrl" => (Scalar::Url, "QUrl"),
            "color" | "QColor" => (Scalar::Color, "QColor"),
            "date" | "QDateTime" => (Scalar::Date, "QDateTime"),
            _ => return None,
        })
    }

    fn new(kind: SlotKind, to: impl Into<String>) -> Slot {
        Slot {
            kind,
            to: to.into(),
            rule: SlotRule::Property,
        }
    }

    /// Of a property a QML document declares with type `ty`.
    pub fn of_qml(ty: &str) -> Slot {
        if let Some(elem) = ty.strip_prefix("list<").and_then(|t| t.strip_suffix('>')) {
            let (elem, to) = match elem {
                "int" => ("int", "QList<int>"),
                "real" | "double" => ("real", "QList<double>"),
                "bool" => ("bool", "QList<bool>"),
                "string" => ("string", "QStringList"),
                "url" => ("url", "QList<QUrl>"),
                "color" => ("color", "QList<QColor>"),
                "var" | "variant" => ("var", "QVariantList"),
                elem if Slot::scalar(elem).is_some() || Vt::of_type(elem).is_some() => {
                    return Slot::new(SlotKind::Unknown, "");
                }
                elem => {
                    let elem = crate::runtime::type_of(elem);
                    let reg = crate::types::registry();
                    let class = reg.resolve(elem).map_or(elem, |(_, t)| t.cpp.as_str());
                    return Slot::new(
                        SlotKind::Objects(elem.into()),
                        format!("QQmlListProperty<{class}>"),
                    );
                }
            };
            return Slot::new(SlotKind::List(elem), to);
        }
        if let Some((scalar, to)) = Slot::scalar(ty) {
            return Slot::new(SlotKind::Scalar(scalar), to);
        }
        if let Some(vt) = Vt::of_type(ty) {
            return Slot::new(SlotKind::Value(vt), vt.cpp());
        }
        match ty {
            "var" | "variant" | "list" | "" => Slot::new(SlotKind::Var, "QVariant"),
            _ => {
                // `P.Part` behind an import qualifier is the type `Part`.
                let ty = crate::runtime::type_of(ty);
                let reg = crate::types::registry();
                let class = reg.resolve(ty).map_or(ty, |(_, t)| t.cpp.as_str());
                Slot::new(SlotKind::Object(ty.into()), format!("{class}*"))
            }
        }
    }

    /// Of a C++ property as `.qmltypes` describes it; `names` are the QML types of the
    /// object that has it, which name its enum's keys.
    pub fn of_cpp(pi: &crate::types::PropInfo, names: &[crate::runtime::Sym]) -> Slot {
        let rule = match pi.qproperty {
            true => SlotRule::Bindable,
            false => SlotRule::Property,
        };
        let ty = pi.ty.as_str();
        let slot = if pi.list && pi.readonly {
            // A QQmlListProperty: its objects are of the C++ class.
            Slot::new(
                SlotKind::Objects(ty.into()),
                format!("QQmlListProperty<{ty}>"),
            )
        } else if pi.list {
            let (elem, to) = match ty {
                "int" => ("int", "QList<int>"),
                "bool" => ("bool", "QList<bool>"),
                "double" | "qreal" => ("real", "QList<double>"),
                "QString" => ("string", "QStringList"),
                "QUrl" => ("url", "QList<QUrl>"),
                "QVariant" => ("var", "QVariantList"),
                _ => return Slot::new(SlotKind::Unknown, ""),
            };
            Slot::new(SlotKind::List(elem), to)
        } else if pi.enumeration {
            let reg = crate::types::registry();
            let (scope, name) = match ty.rsplit_once("::") {
                Some((scope, name)) => (Some(scope), name),
                None => (None, ty),
            };
            let is =
                |e: &&crate::types::EnumInfo| e.name == name || e.alias.as_deref() == Some(name);
            let classes = match scope {
                Some(scope) => reg.chain(scope),
                None => (names.iter())
                    .filter_map(|t| reg.resolve(t))
                    .flat_map(|(_, t)| reg.chain(&t.cpp))
                    .collect(),
            };
            let found = (classes.iter()).find_map(|c| c.enums.iter().find(is).map(|e| (c, e)));
            let mut keys: Vec<(String, f64)> = Vec::new();
            for t in names {
                for (of, key, n) in reg.enum_namespace(t) {
                    let named = of.as_deref().is_some_and(|of| match found {
                        Some((_, e)) => of == e.name,
                        None => of == name,
                    });
                    if named && !keys.iter().any(|k| k.0 == key) {
                        keys.push((key, n));
                    }
                }
            }
            let flag = found.is_some_and(|(_, e)| e.flag);
            let to = match found {
                Some((c, e)) if e.flag => {
                    format!(
                        "QFlags<{}::{}>",
                        c.cpp,
                        e.alias.as_deref().unwrap_or(&e.name)
                    )
                }
                Some((c, e)) => format!("{}::{}", c.cpp, e.name),
                None => ty.to_string(),
            };
            let unsigned = !flag && keys.iter().all(|k| k.1 >= 0.0);
            Slot::new(SlotKind::Enum { unsigned, keys }, to)
        } else if let Some(class) = ty.strip_suffix('*') {
            Slot::new(SlotKind::Object(class.into()), ty)
        } else if let Some((scalar, to)) = Slot::scalar(ty) {
            Slot::new(SlotKind::Scalar(scalar), to)
        } else if let Some(vt) = Vt::of_type(ty) {
            Slot::new(SlotKind::Value(vt), vt.cpp())
        } else {
            match ty {
                "QStringList" => Slot::new(SlotKind::List("string"), ty),
                "QVariantList" => Slot::new(SlotKind::List("var"), ty),
                "QVariant" => Slot::new(SlotKind::Variant, "QVariant"),
                "QJSValue" | "" | "var" => Slot::new(SlotKind::Var, "QVariant"),
                "QQuickAnchorLine" => Slot::new(SlotKind::AnchorLine, ty),
                "QQmlScriptString" => Slot::new(SlotKind::ScriptString, ty),
                _ => Slot::new(SlotKind::Unknown, ty),
            }
        };
        Slot { rule, ..slot }
    }

    /// Of property `field` of a value type property (`None`: of a `color`).
    pub fn of_field(vt: Option<Vt>, field: &str) -> Option<Slot> {
        let ty = field_type(vt, field)?;
        let slot = match Slot::scalar(ty) {
            Some((scalar, to)) => Slot::new(SlotKind::Scalar(scalar), to),
            None => Slot::new(SlotKind::Var, "QVariant"),
        };
        Some(Slot {
            rule: SlotRule::Field,
            ..slot
        })
    }
}

/// The value a property did not take, as Qt's two messages name it.
enum Refused {
    /// A type both messages name the same.
    Type(String),
    Null,
    Function,
    /// An object: a script's write names it `QObject*`, a binding by its class.
    Object(Value),
    /// A QML type's name (`Item`), which a binding holds as no value.
    TypeWrapper,
}

fn refused(v: &Value) -> Refused {
    let name = match v {
        Value::Null => return Refused::Null,
        Value::Native(_) => return Refused::Function,
        Value::Item(_) => return Refused::Object(v.clone()),
        Value::Symbol(_) => "QJSValue".into(),
        Value::Str16(_) => "QString".into(),
        Value::Color(_) => "QColor".into(),
        // Script's -0 is no int.
        Value::Num(n) if *n == 0.0 && n.is_sign_negative() => "double".into(),
        Value::Obj(o) => {
            let b = o.borrow();
            match b.internal {
                _ if crate::types::is_type_wrapper(&b) => return Refused::TypeWrapper,
                Internal::RegExp { .. } => "QRegularExpression".into(),
                Internal::AnchorLine(_) => "QQuickAnchorLine".into(),
                Internal::ValueType(_) | Internal::Url(..) | Internal::Date(_) => type_name(v),
                _ if b.call.is_some() => return Refused::Function,
                _ => "QJSValue".into(),
            }
        }
        v => type_name(v),
    };
    Refused::Type(name)
}

fn is_function(v: &Value) -> bool {
    match v {
        Value::Native(_) => true,
        Value::Obj(o) => {
            let b = o.borrow();
            b.call.is_some() && !crate::types::is_type_wrapper(&b)
        }
        _ => false,
    }
}

/// A QString made of a list, which is a sequence of QChar to QMetaSequence: a number is the
/// character of that code, a string its first character, anything else the null character.
fn string_of_list(list: &[Value]) -> Value {
    let units: Vec<u16> = (list.iter())
        .map(|e| match e {
            Value::Num(n) => *n as u16,
            Value::Str(s) => s.encode_utf16().next().unwrap_or(0),
            Value::Obj(o) => match o.borrow().array.as_deref() {
                Some([Value::Num(n)]) => *n as u16,
                _ => 0,
            },
            _ => 0,
        })
        .collect();
    Value::from_utf16(&units)
}

/// `val` as the property of `slot` takes it from a script (qtdeclarative v6.12.0,
/// src/qml/jsruntime/qv4qobjectwrapper.cpp: QV4::QObjectWrapper::setProperty) or, with
/// `binding`, as the result of its binding (src/qml/qml/qqmlbinding.cpp: QQmlBinding::
/// slowWrite and the bindings of one type): an `int` takes a number by ToInt32, a `real`
/// and a `bool` what script converts to one, any other type the QVariant of the value
/// (QV4::ExecutionEngine::toVariant with the property's type as the hint) by the rule of
/// a write from C++. `undefined` is the caller's.
fn slot_value(
    it: &mut crate::eval::Interp,
    slot: &Slot,
    val: Value,
    binding: bool,
) -> crate::eval::R<Result<Value, Refused>> {
    use SlotKind as K;
    let scalar = match &slot.kind {
        K::Var | K::Unknown => return Ok(Ok(val)),
        // A line alone converts to a line.
        K::AnchorLine => {
            return Ok(match &val {
                Value::Obj(o) if matches!(o.borrow().internal, Internal::AnchorLine(_)) => Ok(val),
                _ if is_function(&val) => Err(Refused::Function),
                _ => Err(refused(&val)),
            });
        }
        // What a document binds to a QQmlScriptString is its expression, no binding.
        K::ScriptString if binding => return Ok(Ok(val)),
        // A script makes none: Qt takes a primitive for the expression and refuses an object.
        K::ScriptString => {
            return Ok(match &val {
                Value::Obj(_) | Value::Item(_) | Value::Native(_) | Value::Color(_) => {
                    Err(refused(&val))
                }
                _ => Ok(val),
            });
        }
        // A QML type's name is no value.
        K::Variant => match &val {
            Value::Obj(o) if crate::types::is_type_wrapper(&o.borrow()) => {
                return Ok(Ok(Value::Null));
            }
            _ if is_function(&val) => return Ok(Err(Refused::Function)),
            _ => return Ok(Ok(val)),
        },
        K::Scalar(s) => Some(*s),
        _ => None,
    };
    // What a QQmlPropertyBinding gives is converted as script converts it.
    let coerced = binding && slot.rule == SlotRule::Bindable;
    if is_function(&val) {
        let taken = binding
            && slot.rule != SlotRule::Field
            && (scalar == Some(Scalar::Bool) || (coerced && scalar.is_some()));
        if !taken {
            return Ok(Err(Refused::Function));
        }
    }
    let field = match (&slot.kind, scalar) {
        _ if slot.rule != SlotRule::Field => None,
        // An enum is written as the int it is.
        (K::Enum { .. }, _) | (_, Some(Scalar::Int)) => Some("int"),
        (_, Some(Scalar::Real)) => Some("real"),
        (_, Some(Scalar::String)) => Some("string"),
        (_, Some(Scalar::Url)) => Some("url"),
        (_, Some(Scalar::Color)) => Some("color"),
        (_, Some(Scalar::Date)) => Some("date"),
        _ => None,
    };
    if let (K::Enum { .. }, Some(_)) = (&slot.kind, field) {
        match &val {
            // A binding's NaN is no enum.
            Value::Num(n) if n.is_nan() && binding => return Ok(Err(refused(&val))),
            // A double is rounded; one that is no int is INT_MIN (x86-64's cvtsd2si).
            Value::Num(n) => {
                let r = n.round();
                let fits = r >= f64::from(i32::MIN) && r <= f64::from(i32::MAX);
                return Ok(Ok(Value::Num(if fits { r } else { f64::from(i32::MIN) })));
            }
            // A binding's string is no enum.
            Value::Str(_) | Value::Str16(_) if binding => return Ok(Err(refused(&val))),
            _ => {}
        }
    }
    // A script's string is an int when it is written as one.
    if field == Some("int")
        && !binding
        && let Value::Str(s) = &val
    {
        return Ok(match s.trim().parse::<i32>() {
            Ok(n) => Ok(Value::Num(f64::from(n))),
            Err(_) => Err(refused(&val)),
        });
    }
    // A list is the string of its characters to a field too, and a symbol none.
    if field == Some("string") {
        let (list, date) = match &val {
            Value::Obj(o) => {
                let b = o.borrow();
                (b.array.clone(), matches!(b.internal, Internal::Date(_)))
            }
            Value::Symbol(_) => return Ok(Err(refused(&val))),
            _ => (None, false),
        };
        if let Some(list) = list {
            return Ok(Ok(string_of_list(&list)));
        }
        // A Date is the string script makes of it.
        if date {
            return Ok(Ok(Value::str(&it.to_string(&val)?)));
        }
    }
    if let Some(ty) = field {
        return Ok(
            match qqml_property_write(it, &WriteTarget::Type(ty), &val)? {
                Some(v) => Ok(v),
                None => Err(refused(&val)),
            },
        );
    }
    let url = url_of(&val).filter(|_| matches!(val, Value::Obj(_)));
    Ok(match &slot.kind {
        K::Var | K::Variant | K::Unknown | K::AnchorLine | K::ScriptString => Ok(val),
        K::Scalar(Scalar::Int) => match &val {
            Value::Num(n) => Ok(Value::Num(f64::from(crate::eval::to_i32(*n)))),
            Value::Bool(_) | Value::Str(_) => match variant_int(&val) {
                Some(n) => Ok(Value::Num(n)),
                None => Err(refused(&val)),
            },
            _ => Err(refused(&val)),
        },
        K::Scalar(Scalar::Real) => match &val {
            Value::Num(_) => Ok(val),
            Value::Symbol(_) => return it.throw("TypeError", "Type error"),
            _ if url.is_some() && !coerced => Err(Refused::Type("QUrl".into())),
            _ => Ok(Value::Num(it.to_numeric(&val)?)),
        },
        K::Scalar(Scalar::Bool) => match &val {
            _ if url.is_some() && (!binding || slot.rule == SlotRule::Field) => {
                Err(Refused::Type("QUrl".into()))
            }
            _ => Ok(Value::Bool(val.truthy())),
        },
        K::Scalar(Scalar::String) => match &val {
            Value::Str(_) | Value::Str16(_) => Ok(val),
            _ if coerced => Ok(Value::str(&it.to_string(&val)?)),
            Value::Symbol(_) => Err(refused(&val)),
            Value::Obj(o) => {
                let (list, date, wrapper) = {
                    let b = o.borrow();
                    (
                        b.array.clone(),
                        matches!(b.internal, Internal::Date(_)),
                        crate::types::is_type_wrapper(&b),
                    )
                };
                match list {
                    Some(list) => Ok(string_of_list(&list)),
                    // A Date is the string script makes of it.
                    None if date => Ok(Value::str(&it.to_string(&val)?)),
                    None if wrapper => Err(Refused::TypeWrapper),
                    None => variant_string(&val).ok_or_else(|| refused(&val)),
                }
            }
            _ => variant_string(&val).ok_or_else(|| refused(&val)),
        },
        // A QUrl takes a string or a url.
        K::Scalar(Scalar::Url) => match &val {
            // A url QUrl calls invalid stays the string it was made of: it prints as nothing
            // and is still the url that was parsed.
            Value::Str(s) => Ok(url_written(&url_prop(s)).map_or(val.clone(), Value::Str)),
            Value::Str16(_) => {
                let s = val.to_js_string();
                Ok(url_written(&url_prop(&s)).map_or(val.clone(), Value::Str))
            }
            _ => match url_written(&val) {
                Some(s) => Ok(Value::Str(s)),
                None => Err(refused(&val)),
            },
        },
        // A QColor refuses a symbol, a regular expression and a type's name; an object that
        // is no array, date, url or value type makes a color of its properties, and any
        // other value the color of its name or the invalid one.
        K::Scalar(Scalar::Color) => match &val {
            Value::Color(_) => Ok(val),
            Value::Symbol(_) => Err(refused(&val)),
            Value::Obj(o) => {
                let (refuses, plain) = {
                    let b = o.borrow();
                    (
                        // `Qt` is an object of Qt's, and that is a color that is none.
                        matches!(b.internal, Internal::RegExp { .. })
                            || crate::types::is_type_wrapper(&b) && !crate::types::is_qt_global(&b),
                        matches!(b.internal, Internal::Ordinary) && b.array.is_none(),
                    )
                };
                match () {
                    () if refuses => Err(refused(&val)),
                    () if plain => Ok(color_from_object(it, &val)?),
                    () => Ok(Value::Color(crate::builtins::to_color(&val))),
                }
            }
            _ => Ok(Value::Color(crate::builtins::to_color(&val))),
        },
        // A QDateTime takes an ISO 8601 string or a Date.
        K::Scalar(Scalar::Date) => match &val {
            Value::Str(s) => match crate::builtins::qt_iso_datetime(s) {
                Some(ms) => Ok(crate::builtins::plain_date(ms)),
                None => Err(refused(&val)),
            },
            _ if crate::builtins::date_ms(&val).is_some() => Ok(val),
            _ => Err(refused(&val)),
        },
        K::Value(vt) => {
            let vt = *vt;
            // A value type's property names what an object of script's is: a QVariantMap.
            let refuse = |v: &Value| match refused(v) {
                Refused::Type(t) if t == "QJSValue" && is_script_map(v) => {
                    Refused::Type("QVariantMap".into())
                }
                r => r,
            };
            if let Value::Obj(o) = &val {
                let (plain, refuses) = {
                    let b = o.borrow();
                    let refuses = matches!(
                        b.internal,
                        Internal::Date(_) | Internal::Url(..) | Internal::RegExp { .. }
                    ) || crate::types::is_type_wrapper(&b);
                    // A font is an object with none of another type's properties.
                    let special = match b.internal {
                        Internal::ValueType(k) => {
                            !matches!(k, Vt::Font { .. }) || matches!(vt, Vt::Font { .. })
                        }
                        _ => false,
                    };
                    (b.array.is_none() && !special && !refuses, refuses)
                };
                if refuses {
                    return Ok(Err(refuse(&val)));
                }
                if plain && matches!(vt, Vt::Font { .. }) {
                    return Ok(Ok(font_from_object(it, &val)?));
                }
                if plain && let Some(c) = structured(it, &val, vt)? {
                    return Ok(Ok(from_components(vt, &c)));
                }
            }
            // An object of Qt's, or a color, is read as script's object is: the type's
            // properties it has.
            // The object the engine has for a property of this type (`font: other.font`)
            // is that property's value.
            if let Value::Item(id) = &val
                && it.host.is_instance(*id, vt.cpp())
            {
                return Ok(Ok(val));
            }
            if matches!(val, Value::Item(_) | Value::Color(_)) {
                if matches!(vt, Vt::Font { .. }) {
                    return Ok(Ok(font_from_object(it, &val)?));
                }
                let mut c = defaults(vt);
                for (i, k) in vt.data().iter().enumerate() {
                    let e = it.get(&val, k)?;
                    if !matches!(e, Value::Undefined) {
                        c[i] = it.to_numeric(&e)?;
                    }
                }
                return Ok(Ok(from_components(vt, &c)));
            }
            convert_plain(&val, vt).ok_or_else(|| refuse(&val))
        }
        // A list of colors or urls is the list script gives, each value in it the color or
        // url it converts to, or the invalid color or empty url. A string is a list of its
        // characters to a color, one url to a url; of any other value a color makes the
        // one invalid color and a url nothing.
        K::List(elem @ ("color" | "url")) => {
            let color = *elem == "color";
            let one = |v: &Value| match (color, v) {
                (true, Value::Color(_)) => v.clone(),
                (true, Value::Str(_)) => Value::Color(crate::builtins::to_color(v)),
                (true, _) => Value::Color(crate::builtins::to_color(&Value::str("?"))),
                (false, Value::Str(s)) => url_prop(s),
                (false, v) => make_url(
                    &url_of(v)
                        .filter(|_| matches!(v, Value::Obj(_)))
                        .unwrap_or_default(),
                ),
            };
            let list = match &val {
                Value::Obj(o) => o.borrow().array.clone(),
                _ => None,
            };
            let list: Vec<Value> = match (&val, list) {
                (_, Some(list)) => list.iter().map(one).collect(),
                (Value::Color(_), _) if color => vec![val.clone()],
                (Value::Str(s), _) if color => {
                    s.encode_utf16().map(|_| one(&Value::Null)).collect()
                }
                (Value::Str(_), _) => vec![one(&val)],
                _ if color || url.is_some() => vec![one(&val)],
                _ => Vec::new(),
            };
            Ok(Value::array(list))
        }
        // A list of script's is converted as script converts (QV4::SequencePrototype): each
        // value to a number, a boolean or a string. A string is the list of its characters,
        // which are no numbers. Any other value makes the list a write from C++ makes.
        K::List(elem) => {
            let list = match &val {
                Value::Obj(o) => o.borrow().array.clone(),
                _ => None,
            };
            match (&val, list) {
                (_, Some(list)) => {
                    let mut out = Vec::with_capacity(list.len());
                    for e in list {
                        out.push(match *elem {
                            "int" => match &e {
                                Value::Symbol(_) => Value::Num(0.0),
                                e => Value::Num(f64::from(crate::eval::to_i32(it.to_numeric(e)?))),
                            },
                            "real" => match &e {
                                Value::Symbol(_) => Value::Num(f64::NAN),
                                e => Value::Num(it.to_numeric(e)?),
                            },
                            "bool" => Value::Bool(e.truthy()),
                            "string" => match &e {
                                Value::Str(_) | Value::Str16(_) => e,
                                Value::Symbol(_) => Value::str(""),
                                e => Value::str(&it.to_string(e)?),
                            },
                            _ => e,
                        });
                    }
                    Ok(Value::array(out))
                }
                (Value::Str(s), _) if !matches!(*elem, "int" | "string") => {
                    let chars = s.chars().map(|c| match *elem {
                        "real" => Value::Num(0.0),
                        "bool" => Value::Bool(true),
                        _ => Value::str(c.encode_utf8(&mut [0; 4])),
                    });
                    Ok(Value::array(chars.collect()))
                }
                // A QML type's name is no value, and a symbol no string.
                (Value::Obj(o), _)
                    if *elem == "var" && crate::types::is_type_wrapper(&o.borrow()) =>
                {
                    Ok(Value::array(vec![Value::Null]))
                }
                (Value::Symbol(_), _) if *elem == "string" => {
                    Ok(Value::array(vec![Value::str("")]))
                }
                _ => Ok(variant_list(elem, &val)),
            }
        }
        // A QQmlListProperty takes the objects of a list, or the one object, and a null for
        // anything else.
        K::Objects(class) => {
            let list = match &val {
                Value::Obj(o) => o.borrow().array.clone(),
                _ => None,
            };
            let list = (list.unwrap_or_else(|| vec![val.clone()]).into_iter())
                .map(|e| match e {
                    Value::Item(id) if it.host.is_instance(id, class) => e,
                    _ => Value::Null,
                })
                .collect();
            Ok(Value::array(list))
        }
        K::Enum { unsigned, keys } => {
            let n = match &val {
                Value::Num(_) | Value::Bool(_) => variant_int(&val),
                Value::Str(s) => keys.iter().find(|k| k.0 == **s).map(|k| k.1),
                _ => None,
            };
            match n {
                Some(n) if *unsigned && n < 0.0 => Ok(Value::Num(n + 4294967296.0)),
                Some(n) => Ok(Value::Num(n)),
                None => Err(refused(&val)),
            }
        }
        K::Object(class) => match &val {
            Value::Null => Ok(val),
            Value::Item(id) if it.host.is_instance(*id, class) => Ok(val),
            Value::Obj(o) if crate::types::is_type_wrapper(&o.borrow()) => Ok(Value::Null),
            // What the engine has in place of a QObject of Qt (a SpinBox's validator).
            Value::Obj(o) if stands_for_qobject(&o.borrow()) => Ok(val),
            _ => Err(refused(&val)),
        },
    })
}

/// Whether a script object is the engine's own, in place of an object of Qt: it has Rust
/// data behind it or a key no script can write.
fn stands_for_qobject(o: &crate::value::JsObject) -> bool {
    o.native.is_some() || o.props.keys().any(|k| k.starts_with('\x01'))
}

/// `val` assigned from script to the property of `slot` (QV4::QObjectWrapper::
/// setProperty): the value the property takes ([`slot_value`]), or the error Qt throws
/// for one it refuses, "Cannot assign". A write from C++ is another rule,
/// [`qqml_property_write`].
pub fn qobject_wrapper_set_property(
    it: &mut crate::eval::Interp,
    slot: &Slot,
    val: Value,
) -> crate::eval::R<Value> {
    // A Qt.binding() is the property's new binding, whatever the type.
    if crate::builtins::binding_target(&val).is_some() {
        return Ok(val);
    }
    let from = match slot_value(it, slot, val, false)? {
        Ok(v) => return Ok(v),
        Err(Refused::Type(t)) => t,
        Err(Refused::Null) => "std::nullptr_t".into(),
        // QQmlValueTypeWrapper::virtualPut throws this string, not an Error.
        Err(Refused::Function) if slot.rule == SlotRule::Field => {
            let m = "Cannot assign JavaScript function to value-type property";
            return Err(crate::eval::Ex::Throw(Value::str(m)));
        }
        Err(Refused::Function) => "JavaScript function".into(),
        Err(Refused::Object(_) | Refused::TypeWrapper) => "QObject*".into(),
    };
    it.throw("Error", format!("Cannot assign {from} to {}", slot.to))
}

/// The result `val` of the binding of the property of `slot`: the value the property
/// takes ([`slot_value`]), or what Qt logs after the binding's place for one it refuses,
/// while the property keeps its value.
pub fn binding_result(
    it: &mut crate::eval::Interp,
    slot: &Slot,
    val: Value,
) -> crate::eval::R<Result<Value, String>> {
    let (from, object) = match slot_value(it, slot, val, true)? {
        Ok(v) => return Ok(Ok(v)),
        Err(Refused::Function) => {
            let m = "Unable to assign a function to a property of any type other than var.";
            return Ok(Err(m.into()));
        }
        Err(Refused::Type(t)) => (t, false),
        Err(Refused::Null) => ("null".into(), false),
        Err(Refused::TypeWrapper) => ("undefined".into(), false),
        // QObject::toString is the class and the address.
        Err(Refused::Object(o)) => {
            let s = it.to_string(&o)?;
            (s.split('(').next().unwrap_or_default().to_string(), true)
        }
    };
    // Of two classes Qt names neither as a pointer.
    let to = match object {
        true => slot.to.trim_end_matches('*'),
        false => &slot.to,
    };
    Ok(Err(format!("Unable to assign {from} to {to}")))
}
