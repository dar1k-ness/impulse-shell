//! The members of the Qt 6.12 `Qt` global object (QQmlEngine's `QtObject`, "Qt QML Type") that
//! are plain functions of their arguments.

use crate::color::Color;
use crate::eval::{Ex, Interp, R, Scope, ScriptFile, new_obj, set_own};
use crate::value::Value;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// QV4's argument-count check of a Q_INVOKABLE with exactly `n` parameters.
pub(crate) fn arity(it: &Interp, a: &[Value], n: usize) -> R<()> {
    arity_range(it, a, n, n)
}

/// As `arity`, for an invokable whose overloads take `min..=max` arguments.
pub(crate) fn arity_range(it: &Interp, a: &[Value], min: usize, max: usize) -> R<()> {
    if a.len() < min {
        it.throw("Error", "Insufficient arguments")
    } else if a.len() > max {
        it.throw("Error", "Too many arguments")
    } else {
        Ok(())
    }
}

/// QV4's argument-count check of a Q_INVOKABLE with one signature of `n` parameters: fewer
/// arguments are "Insufficient", more are "Too many" unless the extra ones are `undefined`
/// (QV4 does not count trailing `undefined` arguments).
pub(crate) fn invokable(it: &Interp, a: &[Value], n: usize) -> R<()> {
    if a.len() < n {
        it.throw("Error", "Insufficient arguments")
    } else if defined(a) > n {
        it.throw("Error", "Too many arguments")
    } else {
        Ok(())
    }
}

/// The count of arguments without the trailing `undefined` ones.
fn defined(a: &[Value]) -> usize {
    a.iter()
        .rposition(|v| !matches!(v, Value::Undefined))
        .map_or(0, |i| i + 1)
}

/// QV4's choice between the two overloads of a Q_INVOKABLE, of `n` and of `n + 1`
/// parameters: how many arguments the chosen one takes. Exactly `n` arguments take the
/// first; more take it when only `n` are left without the trailing `undefined` ones, and
/// the second otherwise. Fewer than `n` match no overload: the error lists `candidates`.
pub(crate) fn overload(it: &Interp, a: &[Value], n: usize, candidates: &str) -> R<usize> {
    if a.len() < n {
        return it.throw(
            "Error",
            format!("Unable to determine callable overload.  Candidates are:\n{candidates}"),
        );
    }
    let defined = defined(a);
    if a.len() == n || defined == n {
        Ok(n)
    } else if defined > n + 1 {
        it.throw("Error", "Too many arguments")
    } else {
        Ok(n + 1)
    }
}

/// `qBound(0.0, x, 1.0)`: NaN gives 0.
fn bound01(x: f64) -> f32 {
    if x.is_nan() {
        0.0
    } else {
        x.clamp(0.0, 1.0) as f32
    }
}

/// The three or four `double` arguments of `Qt.rgba`, `Qt.hsla` and `Qt.hsva`, the alpha 1
/// when the three-parameter overload is called.
fn components(it: &mut Interp, a: &[Value], name: &str) -> R<[f64; 4]> {
    let candidates =
        format!("    {name}(double,double,double)\n    {name}(double,double,double,double)");
    let n = overload(it, a, 3, &candidates)?;
    let mut v = [1.0; 4];
    for i in 0..n {
        v[i] = it.to_numeric(&a[i])?;
    }
    Ok(v)
}

/// `Qt.rgba(red, green, blue, alpha = 1)`, each bounded to [0, 1].
pub fn rgba(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let [r, g, b, alpha] = components(it, a, "rgba")?.map(bound01);
    Ok(Value::Color(Color::from_rgb_f(r, g, b, alpha)))
}

/// The hue as QtObject::hsla and hsva bound it: -1 (achromatic) stays.
fn hue(h: f64) -> f32 {
    if h == -1.0 { -1.0 } else { bound01(h) }
}

/// `Qt.hsla(hue, saturation, lightness, alpha = 1)`.
pub fn hsla(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let [h, s, l, alpha] = components(it, a, "hsla")?;
    Ok(Value::Color(Color::from_hsl_f(
        hue(h),
        bound01(s),
        bound01(l),
        bound01(alpha),
    )))
}

/// `Qt.hsva(hue, saturation, value, alpha = 1)`.
pub fn hsva(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let [h, s, v, alpha] = components(it, a, "hsva")?;
    Ok(Value::Color(Color::from_hsv_f(
        hue(h),
        bound01(s),
        bound01(v),
        bound01(alpha),
    )))
}

/// `Qt.color(name)`: the color `name` (as a string) names; any other string throws.
pub fn color(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    invokable(it, a, 1)?;
    let name = it.to_string(&a[0])?;
    match crate::builtins::parse_color(&name) {
        c if c.spec != crate::color::Spec::Invalid => Ok(Value::Color(c)),
        _ => it.throw("Error", format!("\"{name}\" is not a valid color name")),
    }
}

/// The color of a `QJSValue` parameter of `Qt.lighter`, `Qt.darker` and `Qt.alpha`
/// (colorVariantFromJSValue): a color value, or the color a string names. Anything else
/// makes the call return `null`.
fn base_color(v: &Value) -> Option<Color> {
    match v {
        Value::Color(c) => Some(*c),
        Value::Str(s) => {
            Some(crate::builtins::parse_color(s)).filter(|c| c.spec != crate::color::Spec::Invalid)
        }
        _ => None,
    }
}

/// `int(qRound(factor * 100.))` as Qt 6.12 computes it on x86-64: a value outside `int`
/// converts to `INT_MIN`, which QColor::lighter and darker take as "no change".
fn percent(factor: f64) -> i32 {
    let p = factor * 100.0;
    let r = if p >= 0.0 { p + 0.5 } else { p - 0.5 }.trunc();
    if (-2147483648.0..=2147483647.0).contains(&r) {
        r as i32
    } else {
        i32::MIN
    }
}

/// `Qt.lighter(baseColor, factor = 1.5)` and `Qt.darker(baseColor, factor = 2.0)`
/// (QQuickColorValueType::lighter, darker): `null` for a base that is no color, the
/// invalid color for a NaN factor.
fn shade(it: &mut Interp, a: &[Value], name: &str, default: f64) -> R<Value> {
    let candidates = format!("    {name}(QJSValue)\n    {name}(QJSValue,double)");
    let factor = match overload(it, a, 1, &candidates)? {
        1 => default,
        _ => it.to_numeric(&a[1])?,
    };
    let Some(c) = base_color(&a[0]) else {
        return Ok(Value::Null);
    };
    Ok(Value::Color(if factor.is_nan() {
        Color::INVALID
    } else if name == "lighter" {
        c.lighter(percent(factor))
    } else {
        c.darker(percent(factor))
    }))
}

pub fn lighter(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    shade(it, a, "lighter", 1.5)
}

pub fn darker(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    shade(it, a, "darker", 2.0)
}

/// `Qt.alpha(baseColor, value)`: `baseColor` with the alpha `value`, bounded to [0, 1]
/// (QColor::setAlphaF); `null` for a base that is no color.
pub fn alpha(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    invokable(it, a, 2)?;
    let value = it.to_numeric(&a[1])?;
    let Some(c) = base_color(&a[0]) else {
        return Ok(Value::Null);
    };
    Ok(Value::Color(c.with_alpha_f(value as f32)))
}

/// `Qt.md5(data)`: the MD5 of the UTF-8 of `data` as a string, in hex. An ArrayBuffer
/// converts to the string its bytes are the UTF-8 of (QString::fromUtf8), so the hash of
/// well-formed UTF-8 bytes is the hash of the bytes. A lone surrogate has no UTF-8:
/// QString::toUtf8 leaves it out.
pub fn md5(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    invokable(it, a, 1)?;
    let bytes = match &a[0] {
        Value::Obj(o) => match &o.borrow().internal {
            crate::value::Internal::ArrayBuffer(d) => Some(d.borrow().clone()),
            _ => None,
        },
        _ => None,
    };
    let text = match bytes {
        Some(b) => qstring_from_utf8(&b),
        None => match it.to_str_value(&a[0])? {
            Value::Str16(u) => char::decode_utf16(u.iter().copied())
                .filter_map(Result::ok)
                .collect(),
            s => s.to_js_string(),
        },
    };
    Ok(Value::str(&crate::builtins::md5_hex(text.as_bytes())))
}

/// `Qt.resolvedUrl(url)` and `Qt.resolvedUrl(url, context)` (QtObject::resolvedUrl): `url`
/// resolved in the context of the running code, or in the one `context` was created in, as
/// a `url` value. Without a context object, and in code no context runs, it is resolved
/// against the engine's base url.
pub fn resolved_url(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    use crate::qurl::{resolved_in, resolved_without_context};
    let n = overload(
        it,
        a,
        1,
        "    resolvedUrl(QUrl,QObject*)\n    resolvedUrl(QUrl)",
    )?;
    let incompatible = |it: &Interp| {
        it.throw(
            "TypeError",
            "Passing incompatible arguments to C++ functions from JavaScript is not allowed.",
        )
    };
    let url = match (&a[0], crate::valuetypes::url_of(&a[0])) {
        (_, Some(u)) => u,
        (Value::Str(s), _) => s.to_string(),
        _ => return incompatible(it),
    };
    let root = crate::runtime::shell_root(it);
    let resolved = match (n, a.get(1)) {
        (1, _) => match crate::runtime::calling_url(it) {
            Some(doc) => resolved_in(doc.as_ref(), &url, root.as_deref()),
            None => resolved_without_context(&url),
        },
        (_, Some(Value::Item(id))) => match crate::runtime::item_url(it, *id) {
            Some(doc) => resolved_in(doc.as_ref(), &url, root.as_deref()),
            None => resolved_without_context(&url),
        },
        (_, Some(Value::Null | Value::Undefined)) => resolved_without_context(&url),
        // The `Qt` object is a QObject of no context.
        (_, Some(Value::Obj(o))) if is_qt_global(&o.borrow()) => resolved_without_context(&url),
        _ => return incompatible(it),
    };
    Ok(crate::valuetypes::make_url(&resolved.to_string()))
}

/// `Qt.openUrlExternally(url)` (QtObject::openUrlExternally): the parameter is a QUrl, so a
/// string or a `url` value, and what is opened is `Qt.resolvedUrl(url)`: an address without
/// a scheme is one of the document of the running code. QDesktopServices::openUrl refuses a
/// url that is not valid, the empty one too.
pub fn open_url_externally(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    invokable(it, a, 1)?;
    let url = match (&a[0], crate::valuetypes::url_of(&a[0])) {
        (_, Some(u)) => u,
        (Value::Str(s), _) => s.to_string(),
        _ => {
            return it.throw(
                "TypeError",
                "Passing incompatible arguments to C++ functions from JavaScript is not allowed.",
            );
        }
    };
    let resolved = match crate::runtime::calling_url(it) {
        Some(doc) => {
            let root = crate::runtime::shell_root(it);
            crate::qurl::resolved_in(doc.as_ref(), &url, root.as_deref())
        }
        None => crate::qurl::resolved_without_context(&url),
    };
    let valid = !resolved.to_string().is_empty();
    Ok(Value::Bool(valid && crate::openurl::open(&resolved)))
}

/// Native payload marking `Qt.platform` (a QObject in Qt).
pub(crate) struct QtPlatform;

/// Whether a write to `k` of `o` throws whatever the strictness: `o` is the `Qt` object or
/// `Qt.platform`, QObjects in Qt, and `k` one of its methods or constant properties
/// (QV4::QObjectWrapper: `Cannot assign to read-only property`).
pub(crate) fn constant_member(o: &crate::value::JsObject, k: &str) -> bool {
    o.native
        .as_ref()
        .is_some_and(|n| n.is::<QtGlobal>() || n.is::<QtPlatform>())
        && o.props.contains_key(k)
}

/// Whether `o` is the `Qt` object: it takes no new property, and loses none.
pub(crate) fn is_qt_global(o: &crate::value::JsObject) -> bool {
    o.native.as_ref().is_some_and(|n| n.is::<QtGlobal>())
}

/// QJSValue -> QColor as QtObject's QColor parameters take it: a color value or a color
/// string; a string that names no color is an "Invalid color name", anything else is invalid.
fn color_arg(it: &Interp, fname: &str, v: &Value) -> R<Color> {
    match v {
        Value::Color(c) => Ok(*c),
        Value::Str(s) => match crate::builtins::to_color(&Value::Str(s.clone())) {
            c if c.spec != crate::color::Spec::Invalid => Ok(c),
            _ => it.throw("Error", format!("Qt.{fname}(): Invalid color name")),
        },
        _ => it.throw("Error", format!("Qt.{fname}(): Invalid arguments")),
    }
}

/// `Qt.colorEqual(lhs, rhs)`: QColor::operator== (same spec, components and alpha).
pub fn color_equal(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    arity(it, a, 2)?;
    let l = color_arg(it, "colorEqual", &a[0])?;
    let r = color_arg(it, "colorEqual", &a[1])?;
    Ok(Value::Bool(l.same_qcolor(r)))
}

/// `Qt.tint(baseColor, tintColor)`: tintColor alpha-composited over baseColor; `null` when
/// either is not a valid color (QQuickColorProvider::tint).
pub fn tint(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    arity(it, a, 2)?;
    let (base, tint) = (
        crate::builtins::to_color(&a[0]),
        crate::builtins::to_color(&a[1]),
    );
    if base.spec == crate::color::Spec::Invalid || tint.spec == crate::color::Spec::Invalid {
        return Ok(Value::Null);
    }
    Ok(Value::Color(tint_color(base, tint)))
}

pub(crate) fn tint_color(base: Color, tint: Color) -> Color {
    let (base, tint) = (base.to_rgb(), tint.to_rgb());
    let a = tint.alpha_f();
    if a == 1.0 {
        return tint;
    }
    if a == 0.0 {
        return base;
    }
    let inv = 1.0 - a;
    let mix = |t: f32, b: f32| t * a + b * inv;
    Color::from_rgb_f(
        mix(tint.red_f(), base.red_f()),
        mix(tint.green_f(), base.green_f()),
        mix(tint.blue_f(), base.blue_f()),
        a + inv * base.alpha_f(),
    )
}

/// `Qt.quaternion(scalar, x, y, z)`.
pub fn quaternion(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    arity_range(it, a, 4, usize::MAX)?;
    let mut v = [0.0; 4];
    for (i, n) in v.iter_mut().enumerate() {
        *n = it.to_numeric(&a[i])?;
    }
    Ok(crate::valuetypes::make_num(
        crate::valuetypes::Vt::Quaternion,
        &v,
    ))
}

/// `Qt.matrix4x4()` (identity), `Qt.matrix4x4(m11, ..., m44)` or `Qt.matrix4x4([16 values])`,
/// row-major.
pub fn matrix4x4(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let vals: Vec<f64> = match a.len() {
        0 => (0..16)
            .map(|i| if i % 5 == 0 { 1.0 } else { 0.0 })
            .collect(),
        1 => {
            let arr = match &a[0] {
                Value::Obj(o) if o.borrow().array.as_ref().is_some_and(|v| v.len() == 16) => {
                    o.borrow().array.clone().unwrap_or_default()
                }
                _ => {
                    return it.throw(
                        "Error",
                        "Qt.matrix4x4(): Invalid argument: not a valid matrix4x4 values array",
                    );
                }
            };
            let mut out = Vec::with_capacity(16);
            for v in &arr {
                out.push(it.to_numeric(v)?);
            }
            out
        }
        16 => {
            let mut out = Vec::with_capacity(16);
            for v in a {
                out.push(it.to_numeric(v)?);
            }
            out
        }
        _ => return it.throw("Error", "Too many arguments"),
    };
    let flags = if a.is_empty() {
        crate::valuetypes::IDENTITY
    } else {
        crate::valuetypes::GENERAL
    };
    Ok(crate::valuetypes::make_num(
        crate::valuetypes::Vt::Matrix4x4 { flags },
        &vals,
    ))
}

/// `Qt.escapeHtml(text)`: Qt::String::toHtmlEscaped (`<`, `>`, `&`, `"`; not `'`).
pub fn escape_html(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    arity(it, a, 1)?;
    let s = it.to_string(&a[0])?;
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    Ok(Value::str(&out))
}

/// `Qt.url(url)`: a `url` value of the string (or url), normalised as QUrl::toString gives it.
pub fn url(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    arity(it, a, 1)?;
    if let Some(u) = crate::valuetypes::url_written(&a[0]) {
        return Ok(crate::valuetypes::url_prop(&u));
    }
    let Value::Str(s) = &a[0] else {
        return it.throw(
            "TypeError",
            "Passing incompatible arguments to C++ functions from JavaScript is not allowed.",
        );
    };
    Ok(crate::valuetypes::url_prop(s))
}

/// `Qt.btoa(data)`. A string (or a Date, as its string) takes the deprecated QString overload:
/// its UTF-8 bytes Base64-encoded, as a string. Anything else takes the Qt 6.11 byte overloads
/// and returns a new ArrayBuffer holding the Base64 text.
pub fn btoa(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    use base64::Engine;
    let enc = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    match base64_arg(it, "btoa", a)? {
        Base64Arg::Text(s) => Ok(Value::str(&enc(s.as_bytes()))),
        Base64Arg::Bytes(b) => Ok(new_array_buffer(it, enc(&b).into_bytes())),
    }
}

/// `Qt.atob(data)`. The deprecated QString overload decodes leniently
/// (QByteArray::fromBase64: characters outside the alphabet, `=` included, are skipped) and
/// reads the bytes as UTF-8 (QString::fromUtf8); the byte overloads decode strictly
/// (AbortOnBase64DecodingErrors) into a new ArrayBuffer, throwing "Invalid character".
pub fn atob(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    match base64_arg(it, "atob", a)? {
        Base64Arg::Text(s) => Ok(Value::str(&qstring_from_utf8(&from_base64_lenient(&s)))),
        Base64Arg::Bytes(b) => match from_base64_strict(&b) {
            Some(d) => Ok(new_array_buffer(it, d)),
            None => throw_invalid_character(it),
        },
    }
}

enum Base64Arg {
    Text(String),
    Bytes(Vec<u8>),
}

/// Overload resolution of QtObject's btoa/atob (QVariantList, QByteArray and QString
/// overloads) and the argument's conversion. Trailing undefined arguments do not count, as in
/// QV4. Strings and Dates take the QString overload, an ArrayBuffer its bytes; null,
/// undefined and the values QV4 cannot convert (RegExp, URL, QObjects, locales) are rejected.
/// Every other value is converted as the documented array-like (the elements below
/// ToLength(`length`) of its ToObject, so a number or boolean is empty), each element a
/// number in [0, 256) or a one-character string below U+0100 (QtObject's
/// convertVariantList); anything else is an "Invalid character". QV4 only reads the elements
/// of a real Array and treats typed arrays, arguments objects and other array-likes as empty.
fn base64_arg(it: &mut Interp, f: &str, a: &[Value]) -> R<Base64Arg> {
    use crate::value::Internal;
    if a.is_empty() {
        return it.throw(
            "Error",
            format!(
                "Unable to determine callable overload.  Candidates are:\n    {f}(QVariantList)\n    {f}(QByteArray)\n    {f}(QString)"
            ),
        );
    }
    let defined = a
        .iter()
        .rposition(|v| !matches!(v, Value::Undefined))
        .map_or(0, |i| i + 1);
    if defined > 1 {
        return it.throw("Error", "Too many arguments");
    }
    let incompatible = |it: &Interp| {
        it.throw(
            "TypeError",
            "Passing incompatible arguments to C++ functions from JavaScript is not allowed.",
        )
    };
    let v = &a[0];
    match v {
        Value::Str(s) => return Ok(Base64Arg::Text(s.to_string())),
        Value::Str16(u) => return Ok(Base64Arg::Text(String::from_utf16_lossy(u))),
        Value::Null | Value::Undefined | Value::Item(_) | Value::Color(_) | Value::Empty => {
            return incompatible(it);
        }
        Value::Num(_) | Value::Bool(_) => return Ok(Base64Arg::Bytes(Vec::new())),
        Value::Symbol(_) => return throw_invalid_character(it),
        Value::Native(_) => {}
        Value::Obj(o) => {
            let b = o.borrow();
            match &b.internal {
                Internal::Date(_) => {
                    drop(b);
                    return Ok(Base64Arg::Text(it.to_string(v)?));
                }
                Internal::ArrayBuffer(d) => return Ok(Base64Arg::Bytes(d.borrow().clone())),
                Internal::DetachedArrayBuffer => return Ok(Base64Arg::Bytes(vec![])),
                Internal::Url(..) | Internal::ValueType(crate::valuetypes::Vt::Font { .. }) => {
                    drop(b);
                    return Ok(Base64Arg::Text(it.to_string(v)?));
                }
                Internal::RegExp { .. } | Internal::Locale(..) | Internal::ValueType(_) => {
                    drop(b);
                    return incompatible(it);
                }
                _ if crate::url::tag(&b) == Some("URL") => {
                    drop(b);
                    return incompatible(it);
                }
                _ => {}
            }
        }
    }
    let len = it.get(v, "length")?;
    let len = it.to_numeric(&len)?;
    let len = if len.is_nan() {
        0.0
    } else {
        len.trunc().clamp(0.0, 9007199254740991.0)
    };
    let mut out = Vec::new();
    let mut i = 0.0;
    while i < len {
        let e = it.get(v, &crate::value::num_to_string(i))?;
        match base64_list_byte(&e) {
            Some(b) => out.push(b),
            None => return throw_invalid_character(it),
        }
        i += 1.0;
    }
    Ok(Base64Arg::Bytes(out))
}

/// One element of QtObject's convertVariantList: an int or double in [0, 256), truncated (a
/// NaN becomes 0, as `char(NaN)`), or a string of one UTF-16 code unit below 256.
fn base64_list_byte(v: &Value) -> Option<u8> {
    match v {
        Value::Num(n) => (!(*n < 0.0 || *n >= 256.0)).then_some(*n as u8),
        Value::Str(s) => {
            let mut u = s.encode_utf16();
            match (u.next(), u.next()) {
                (Some(c), None) => u8::try_from(c).ok(),
                _ => None,
            }
        }
        _ => None,
    }
}

/// QV4's `THROW_DOM(DOMEXCEPTION_INVALID_CHARACTER_ERR, "Invalid character")`: an Error
/// with an own `code` of 5.
fn throw_invalid_character<T>(it: &Interp) -> R<T> {
    let e = it.make_error("Error", Some("Invalid character".into()));
    if let Value::Obj(o) = &e {
        set_own(o, "code", Value::Num(5.0));
    }
    Err(crate::eval::Ex::Throw(e))
}

fn new_array_buffer(it: &Interp, bytes: Vec<u8>) -> Value {
    Value::Obj(crate::typedarray::new_buffer(it, bytes).0)
}

/// QByteArray::fromBase64 with the default (non-strict, standard alphabet) options: every
/// character outside the alphabet is skipped.
fn from_base64_lenient(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut buf, mut nbits) = (0u32, 0u32);
    for d in s.bytes().filter_map(base64_digit) {
        buf = (buf << 6) | u32::from(d);
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out.push((buf >> nbits) as u8);
            buf &= (1 << nbits) - 1;
        }
    }
    out
}

/// QByteArray::fromBase64Encoding with AbortOnBase64DecodingErrors: `None` for a character
/// outside the standard alphabet or misplaced padding. Unpadded input is accepted; padding
/// is one or two `=` at the end of input whose length is a multiple of 4.
fn from_base64_strict(b: &[u8]) -> Option<Vec<u8>> {
    let pad = match b {
        [.., b'=', b'='] => 2,
        [.., b'='] => 1,
        _ => 0,
    };
    if pad > 0 && !b.len().is_multiple_of(4) {
        return None;
    }
    let body = &b[..b.len() - pad];
    if body.iter().any(|&c| base64_digit(c).is_none()) {
        return None;
    }
    Some(from_base64_lenient(std::str::from_utf8(body).ok()?))
}

/// QString::fromUtf8: a leading byte order mark is dropped and every byte of an ill-formed
/// sequence becomes its own U+FFFD.
fn qstring_from_utf8(b: &[u8]) -> String {
    let b = b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(b);
    let mut s = String::with_capacity(b.len());
    for chunk in b.utf8_chunks() {
        s.push_str(chunk.valid());
        s.extend(std::iter::repeat_n('\u{FFFD}', chunk.invalid().len()));
    }
    s
}

fn base64_digit(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// `Qt.font(fontSpecifier)`: the application font (the native shell's system font) with the
/// recognised subproperties of `fontSpecifier` applied; a subproperty of the wrong type is
/// ignored, and at least one must apply (QQuickFontValueType::create).
pub fn font(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    arity(it, a, 1)?;
    let Value::Obj(spec) = &a[0] else {
        return it.throw("Error", "Qt.font(): Invalid arguments");
    };
    let sys = crate::colorscheme::system_font();
    let mut f = FontSpec {
        family: sys.qt_family().to_string(),
        style_name: String::new(),
        weight: f64::from(sys.weight),
        point_size: sys.point_size,
        pixel_size: sys.pixels().round(),
        ..FontSpec::default()
    };
    let mut any = false;
    let src = Value::Obj(spec.clone());
    let prop = |it: &mut Interp, k: &str| it.get(&src, k);
    if let Value::Str(s) = prop(it, "family")? {
        (f.family, any) = (s.to_string(), true);
    }
    if let Value::Str(s) = prop(it, "styleName")? {
        (f.style_name, any) = (s.to_string(), true);
    }
    if let Value::Bool(b) = prop(it, "bold")? {
        any = true;
        f.weight = if b { 700.0 } else { 400.0 };
    }
    if let Value::Num(n) = prop(it, "weight")? {
        (f.weight, any) = (n.trunc().clamp(1.0, 1000.0), true);
    }
    for (k, slot) in [
        ("italic", &mut f.italic),
        ("underline", &mut f.underline),
        ("overline", &mut f.overline),
        ("strikeout", &mut f.strikeout),
    ] {
        if let Value::Bool(b) = prop(it, k)? {
            (*slot, any) = (b, true);
        }
    }
    // QQuickFontValueType::create sets the pixel size, then the point size through
    // QFont::setPointSize(int): the one set last is the size.
    let mut pixel_sized = false;
    if let Value::Num(n) = prop(it, "pixelSize")? {
        any = true;
        if n.trunc() > 0.0 {
            f.pixel_size = n.trunc();
            f.point_size = f.pixel_size * 72.0 / 96.0;
            pixel_sized = true;
        }
    }
    if let Value::Num(n) = prop(it, "pointSize")? {
        any = true;
        if n.trunc() > 0.0 {
            f.point_size = n.trunc();
            f.pixel_size = (f.point_size * 96.0 / 72.0).trunc();
            pixel_sized = false;
        }
    }
    let spacing = matches!(prop(it, "letterSpacing")?, Value::Num(_));
    for (k, slot) in [
        ("capitalization", &mut f.capitalization),
        ("letterSpacing", &mut f.letter_spacing),
        ("wordSpacing", &mut f.word_spacing),
        ("hintingPreference", &mut f.hinting_preference),
    ] {
        if let Value::Num(n) = prop(it, k)? {
            let n = match k {
                // QFontPrivate::capital is a 3 bit field, QFontDef::hintingPreference a
                // 2 bit one.
                "capitalization" => f64::from(crate::eval::to_i32(n) & 7),
                "hintingPreference" => f64::from(crate::eval::to_i32(n) & 3),
                _ => crate::valuetypes::qfixed(n),
            };
            (*slot, any) = (n, true);
        }
    }
    for (k, slot) in [
        ("kerning", &mut f.kerning),
        ("preferShaping", &mut f.prefer_shaping),
        ("contextFontMerging", &mut f.context_font_merging),
    ] {
        if let Value::Bool(b) = prop(it, k)? {
            (*slot, any) = (b, true);
        }
    }
    // QQuickFontValueType::create sets PreferTypoLineMetrics for `false` and clears it for
    // `true`, as it does PreferNoShaping.
    if let Value::Bool(b) = prop(it, "preferTypoLineMetrics")? {
        (f.prefer_typo_line_metrics, any) = (!b, true);
    }
    let mut maps = [new_obj(), new_obj()];
    for (k, slot) in ["features", "variableAxes"]
        .into_iter()
        .zip(maps.iter_mut())
    {
        if let Value::Obj(o) = prop(it, k)? {
            let copy = new_obj();
            let keys = o.borrow().keys.clone();
            for key in keys {
                let v = it.get(&Value::Obj(o.clone()), &key)?;
                if let Value::Num(n) = v {
                    set_own(&copy, &key, Value::Num(n));
                }
            }
            (*slot, any) = (copy, true);
        }
    }
    if !any {
        return it.throw(
            "Error",
            "Qt.font(): Invalid argument: no valid font subproperties specified",
        );
    }
    Ok(font_value(
        &f,
        maps,
        crate::valuetypes::Vt::Font {
            pixel_sized,
            spacing,
            raw: crate::valuetypes::FontRaw::DEFAULT,
        },
    ))
}

/// The application font (QGuiApplication::font()), a `font` property's default.
pub fn system_font_value() -> Value {
    let sys = crate::colorscheme::system_font();
    let f = FontSpec {
        family: sys.qt_family().to_string(),
        weight: f64::from(sys.weight),
        point_size: sys.point_size,
        pixel_size: sys.pixels().trunc(),
        ..FontSpec::default()
    };
    font_value(&f, [new_obj(), new_obj()], crate::valuetypes::Vt::FONT)
}

fn font_value(f: &FontSpec, maps: [crate::value::Obj; 2], vt: crate::valuetypes::Vt) -> Value {
    let [features, axes] = maps;
    crate::valuetypes::make(
        vt,
        [
            Value::str(&f.family),
            Value::str(&f.style_name),
            Value::Bool(f.weight > 500.0),
            Value::Num(f.weight),
            Value::Bool(f.italic),
            Value::Bool(f.underline),
            Value::Bool(f.overline),
            Value::Bool(f.strikeout),
            Value::Num(f.point_size),
            Value::Num(f.pixel_size),
            Value::Num(f.capitalization),
            Value::Num(f.letter_spacing),
            Value::Num(f.word_spacing),
            Value::Num(f.hinting_preference),
            Value::Bool(f.kerning),
            Value::Bool(f.prefer_shaping),
            Value::Obj(features),
            Value::Obj(axes),
            Value::Bool(f.context_font_merging),
            Value::Bool(f.prefer_typo_line_metrics),
        ],
    )
}

struct FontSpec {
    family: String,
    style_name: String,
    weight: f64,
    italic: bool,
    underline: bool,
    overline: bool,
    strikeout: bool,
    point_size: f64,
    pixel_size: f64,
    capitalization: f64,
    letter_spacing: f64,
    word_spacing: f64,
    hinting_preference: f64,
    kerning: bool,
    prefer_shaping: bool,
    context_font_merging: bool,
    prefer_typo_line_metrics: bool,
}

impl Default for FontSpec {
    fn default() -> Self {
        FontSpec {
            family: String::new(),
            style_name: String::new(),
            weight: 400.0,
            italic: false,
            underline: false,
            overline: false,
            strikeout: false,
            point_size: 9.0,
            pixel_size: 12.0,
            capitalization: 0.0,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            hinting_preference: 0.0,
            kerning: true,
            prefer_shaping: true,
            context_font_merging: false,
            prefer_typo_line_metrics: false,
        }
    }
}

fn enum_arg(it: &Interp, a: &[Value]) -> R<std::rc::Rc<dyn std::any::Any>> {
    arity(it, a, 2)?;
    let payload = match &a[0] {
        Value::Obj(o) => o.borrow().native.clone(),
        _ => None,
    };
    match payload {
        Some(p) if p.is::<crate::types::QmlEnum>() => Ok(p),
        _ => it.throw("TypeError", "Invalid first argument, expected enum"),
    }
}

fn no_entry<T>(it: &Interp) -> R<T> {
    it.throw(
        "ReferenceError",
        "Invalid second argument, entry is not defined",
    )
}

/// The enum value argument: ToNumber rounded half away from zero (qRound64), NaN and
/// non-finite values naming no entry.
fn enum_value(it: &mut Interp, v: &Value) -> R<Option<f64>> {
    let n = it.to_numeric(v)?;
    Ok(n.is_finite().then(|| n.round()))
}

/// `Qt.enumStringToValue(enumType, keyName)`.
pub fn enum_string_to_value(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let p = enum_arg(it, a)?;
    let key = it.to_string(&a[1])?;
    let Some(e) = p.downcast_ref::<crate::types::QmlEnum>() else {
        return no_entry(it);
    };
    match e.0.iter().find(|(k, _)| **k == *key) {
        Some((_, n)) => Ok(Value::Num(*n)),
        None => no_entry(it),
    }
}

/// The keys of `e` declared with value `v`, in declaration order.
fn enum_keys(e: &crate::types::QmlEnum, v: Option<f64>) -> Vec<std::rc::Rc<str>> {
    e.0.iter()
        .filter(|(_, n)| Some(*n) == v)
        .map(|(k, _)| k.clone())
        .collect()
}

/// `Qt.enumValueToString(enumType, keyValue)`: the last key declared with that value.
pub fn enum_value_to_string(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let p = enum_arg(it, a)?;
    let v = enum_value(it, &a[1])?;
    let keys = p
        .downcast_ref::<crate::types::QmlEnum>()
        .map(|e| enum_keys(e, v))
        .unwrap_or_default();
    match keys.last() {
        Some(k) => Ok(Value::Str(k.clone())),
        None => no_entry(it),
    }
}

/// `Qt.enumValueToStrings(enumType, keyValue)`: every key declared with that value, in
/// declaration order.
pub fn enum_value_to_strings(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let p = enum_arg(it, a)?;
    let v = enum_value(it, &a[1])?;
    let keys = p
        .downcast_ref::<crate::types::QmlEnum>()
        .map(|e| enum_keys(e, v))
        .unwrap_or_default();
    if keys.is_empty() {
        return no_entry(it);
    }
    Ok(Value::array(keys.into_iter().map(Value::Str).collect()))
}

/// QLocale::bcp47Name of the system locale, the engine's initial `uiLanguage`: the territory is
/// dropped when it is the language's likely one (`en_US` -> `en`), and the C locale is
/// `en-POSIX`.
pub(crate) fn system_ui_language() -> String {
    bcp47_name(&crate::builtins::system_locale_name())
}

fn bcp47_name(posix: &str) -> String {
    if posix == "C" {
        return "en-POSIX".into();
    }
    let mut parts = posix.split('_');
    let lang = parts.next().unwrap_or("");
    let rest: Vec<&str> = parts.collect();
    let territory = rest
        .iter()
        .find(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_uppercase()));
    match territory {
        Some(t) if !crate::locale::is_likely_territory(lang, t) => std::iter::once(lang)
            .chain(rest)
            .collect::<Vec<_>>()
            .join("-"),
        _ => std::iter::once(lang)
            .chain(rest.iter().copied().filter(|p| Some(p) != territory))
            .collect::<Vec<_>>()
            .join("-"),
    }
}

/// QStyleHints as the native shell implements it: its input handling uses Qt's platform
/// defaults (QPlatformIntegration/QPlatformTheme::themeHint), which these are. `colorScheme`
/// and `cursorFlashTime` are the platform theme's (`colorscheme`); `useHoverEffects` is true
/// under every theme Qt has on a Unix desktop (QGenericUnixTheme and the plugins alike).
const STYLE_HINTS: &[(&str, Value)] = &[
    ("fontSmoothingGamma", Value::Num(1.7)),
    ("keyboardAutoRepeatRate", Value::Num(30.0)),
    ("keyboardInputInterval", Value::Num(400.0)),
    ("mouseDoubleClickInterval", Value::Num(400.0)),
    ("mousePressAndHoldInterval", Value::Num(800.0)),
    ("passwordMaskDelay", Value::Num(0.0)),
    ("setFocusOnTouchRelease", Value::Bool(false)),
    ("showIsFullScreen", Value::Bool(false)),
    ("showIsMaximized", Value::Bool(false)),
    ("showShortcutsInContextMenus", Value::Bool(true)),
    ("contextMenuTrigger", Value::Num(0.0)),
    ("startDragDistance", Value::Num(10.0)),
    ("startDragTime", Value::Num(500.0)),
    ("startDragVelocity", Value::Num(0.0)),
    ("useRtlExtensions", Value::Bool(false)),
    ("tabFocusBehavior", Value::Num(255.0)),
    ("singleClickActivation", Value::Bool(false)),
    ("useHoverEffects", Value::Bool(true)),
    ("wheelScrollLines", Value::Num(3.0)),
    ("mouseQuickSelectionThreshold", Value::Num(10.0)),
    ("mouseDoubleClickDistance", Value::Num(5.0)),
    ("touchDoubleTapDistance", Value::Num(10.0)),
    ("menuSelectionWraps", Value::Bool(true)),
];

/// QGuiApplication's style hints, one object for `Qt.styleHints` and `Application.styleHints`.
fn style_hints(w: &mut crate::runtime::World) -> usize {
    if let Some(hints) = w.style_hints {
        return hints;
    }
    let hints = crate::types::native_object(w, "QStyleHints");
    for (k, v) in STYLE_HINTS {
        w.write(hints, k, v.clone());
    }
    w.write(hints, "passwordMaskCharacter", Value::str("\u{25cf}"));
    let scheme = crate::colorscheme::color_scheme();
    w.write(hints, "colorScheme", Value::Num(scheme as i32 as f64));
    let flash = crate::colorscheme::cursor_flash_time();
    w.write(hints, "cursorFlashTime", Value::Num(flash as f64));
    w.style_hints = Some(hints);
    hints
}

/// What QGuiApplication has set on the `Application` singleton `app` before QML reads it.
pub(crate) fn init_application(w: &mut crate::runtime::World, app: usize) {
    let exe = std::env::current_exe().ok();
    let name = exe
        .as_deref()
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let args = std::env::args().map(|s| Value::str(&s)).collect();
    w.write(app, "arguments", Value::array(args));
    w.write(app, "name", Value::str(&name));
    w.write(app, "displayName", Value::str(&name));
    // QGuiApplication::applicationState: the shell is not focus-tracked as an
    // application, so it reports Qt's state with no active window.
    w.write(app, "state", Value::Num(2.0));
    w.write(app, "supportsMultipleWindows", Value::Bool(true));
    let hints = style_hints(w);
    w.write(app, "styleHints", Value::Item(hints));
}

/// The QObject behind the `Qt` global's properties: `objectName` and `uiLanguage` (writable,
/// with their NOTIFY signals), `application` (the `Application` singleton), `styleHints` and
/// `inputMethod`.
pub(crate) fn make_qt_object_item(w: &mut crate::runtime::World) -> usize {
    let id = w.new_inst("QtObject", None);
    init(w, id, "objectName", "string", Value::str(""));
    init(
        w,
        id,
        "uiLanguage",
        "string",
        Value::str(&system_ui_language()),
    );

    let hints = style_hints(w);
    let app = w.type_value("Application").unwrap_or(Value::Null);

    let im = crate::inputmethod::make(w);

    for (k, v) in [
        ("application", app),
        ("styleHints", Value::Item(hints)),
        ("inputMethod", Value::Item(im)),
    ] {
        init(w, id, k, "var", v);
    }
    id
}

/// Declares `name` on `item` with initial value `v`: a constructor's value, not a change.
fn init(w: &mut crate::runtime::World, item: usize, name: &str, ty: &str, v: Value) {
    w.declare(item, name, ty);
    w.items[item].props.entry(name).or_default().value = v;
}

/// The `Qt` object's QObject property `name`: read through the host so bindings track it.
fn qt_prop(it: &mut Interp, name: &str) -> R<Value> {
    let Some(world) = crate::runtime::world_of(it) else {
        return Ok(match name {
            "objectName" => Value::str(""),
            "uiLanguage" => Value::str(&system_ui_language()),
            _ => Value::Null,
        });
    };
    let id = world.borrow_mut().qt_object_item();
    Ok(it.host.get(id, name).unwrap_or_default())
}

/// A QString property write: V4 converts with ToString, except `undefined`.
fn qt_set_string(it: &mut Interp, name: &str, a: &[Value]) -> R<Value> {
    let v = a.first().cloned().unwrap_or_default();
    if matches!(v, Value::Undefined) {
        return it.throw("Error", "Cannot assign [undefined] to QString");
    }
    let s = it.to_string(&v)?;
    if let Some(world) = crate::runtime::world_of(it) {
        let id = world.borrow_mut().qt_object_item();
        it.host.set(id, name, Value::str(&s));
    }
    Ok(Value::Undefined)
}

/// The `Qt` object's QObject signal `name` (`objectNameChanged`), connectable like any signal.
fn qt_signal(it: &mut Interp, name: &'static str) -> R<Value> {
    match crate::runtime::world_of(it) {
        Some(world) => {
            let id = world.borrow_mut().qt_object_item();
            Ok(crate::eval::fn_value(crate::eval::Callable::Item(id, name)))
        }
        None => Ok(Value::Undefined),
    }
}

/// Defines the `Qt` object's QObject member `name` as an enumerable accessor.
pub(crate) fn define_qobject_member(qt: &crate::value::Obj, name: &'static str) {
    use crate::eval::{Callable, NativeFn, define_accessor, fn_value};
    let get: NativeFn = match name {
        "objectName" => |it, _, _| qt_prop(it, "objectName"),
        "uiLanguage" => |it, _, _| qt_prop(it, "uiLanguage"),
        "application" => |it, _, _| qt_prop(it, "application"),
        "styleHints" => |it, _, _| qt_prop(it, "styleHints"),
        "inputMethod" => |it, _, _| qt_prop(it, "inputMethod"),
        "objectNameChanged" => |it, _, _| qt_signal(it, "objectNameChanged"),
        "uiLanguageChanged" => |it, _, _| qt_signal(it, "uiLanguageChanged"),
        _ => unreachable!("not a QObject member of Qt: {name}"),
    };
    define_accessor(qt, name, true, fn_value(Callable::Native(name, get)), true);
    let set: NativeFn = match name {
        "objectName" => |it, _, a| qt_set_string(it, "objectName", a),
        "uiLanguage" => |it, _, a| qt_set_string(it, "uiLanguage", a),
        "application" => |it, _, _| read_only(it, "application"),
        "styleHints" => |it, _, _| read_only(it, "styleHints"),
        "inputMethod" => |it, _, _| read_only(it, "inputMethod"),
        "objectNameChanged" => |it, _, _| read_only(it, "objectNameChanged"),
        _ => |it, _, _| read_only(it, "uiLanguageChanged"),
    };
    define_accessor(qt, name, false, fn_value(Callable::Native(name, set)), true);
}

/// A write to a constant QObject property or a signal: V4 throws whatever the strictness.
fn read_only(it: &mut Interp, name: &str) -> R<Value> {
    it.throw(
        "TypeError",
        format!("Cannot assign to read-only property \"{name}\""),
    )
}

/// `Qt.isQtObject(object)`: whether `object` is a QObject (a QML object, the `Qt` object).
pub fn is_qt_object(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    arity(it, a, 1)?;
    Ok(Value::Bool(match &a[0] {
        Value::Item(_) => true,
        Value::Obj(o) => o
            .borrow()
            .native
            .as_ref()
            .is_some_and(|n| n.is::<QtGlobal>()),
        _ => false,
    }))
}

/// Native payload marking the `Qt` global object itself (a QObject in Qt).
pub(crate) struct QtGlobal;

/// Whether `o` is the `Qt` global object.
pub(crate) fn is_qt(o: &crate::value::JsObject) -> bool {
    o.native.as_ref().is_some_and(|n| n.is::<QtGlobal>())
}

/// `Qt.include(url, callback)` (deprecated in Qt 6 for ECMAScript modules): runs the JavaScript
/// file at `url`, resolved against the calling JS file, in the calling file's context, which
/// gets its declarations, and returns, and passes `callback`, the include result: the `OK`,
/// `LOADING`, `NETWORK_ERROR` and `EXCEPTION` constants, the `status` and, after a throw, the
/// `exception`. Only a JS file can call it. The production build has no code for a file rigen
/// did not compile (it compiles the literal targets of the files it compiles); that include
/// throws an `EXCEPTION`, as a script Qt fails to compile does.
pub fn include(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    if a.is_empty() {
        return it.throw(
            "Error",
            "Unable to determine callable overload.  Candidates are:\n    include(QString,QJSValue)\n    include(QString)",
        );
    }
    let Some((caller, decls)) = it.calling_script() else {
        return it.throw(
            "Error",
            "Qt.include(): Can only be called from JavaScript files",
        );
    };
    let url = a[0].to_js_string();
    let (status, exception) = match resolve_include(&caller.file, &url) {
        Some(path) => run_include(it, path, &decls)?,
        None => (Include::NetworkError, None),
    };
    let result = new_obj();
    for (k, s) in [
        ("OK", Include::Ok),
        ("LOADING", Include::Loading),
        ("NETWORK_ERROR", Include::NetworkError),
        ("EXCEPTION", Include::Exception),
        ("status", status),
    ] {
        set_own(&result, k, Value::Num(s as i32 as f64));
    }
    if let Some(e) = exception {
        set_own(&result, "exception", e);
    }
    let result = Value::Obj(result);
    if let Some(f) = a.get(1).filter(|f| crate::eval::callable(f).is_some()) {
        // QV4Include::callback reports a throwing callback as a warning, not to the caller.
        match it.call(f, Value::Undefined, std::slice::from_ref(&result)) {
            Err(Ex::Throw(e)) => {
                let m = it
                    .get(&e, "message")
                    .map(|v| v.to_js_string())
                    .unwrap_or_default();
                crate::warning::say(Default::default(), &format!("Qt.include: {m}"));
            }
            r => {
                r?;
            }
        }
    }
    Ok(result)
}

/// The include result `status` values (`QV4Include::Status`).
#[derive(Clone, Copy)]
enum Include {
    Ok = 0,
    Loading = 1,
    NetworkError = 2,
    Exception = 3,
}

/// The local file `url` names, relative to the calling file (`QQmlContext::resolvedUrl`); `None`
/// for a URL with another scheme, which only the network could load and the shell does not.
pub fn resolve_include(caller: &Path, url: &str) -> Option<PathBuf> {
    let base = url::Url::from_file_path(caller).ok()?;
    let u = base.join(url).ok()?;
    (u.scheme() == "file").then(|| u.to_file_path().ok())?
}

/// Run the file at `path` in the JS-file context whose declarations live in `decls`, as
/// `QV4Include::method_include` does: a file it cannot read is a `NETWORK_ERROR`, a throw an
/// `EXCEPTION` with the thrown value. The context's namespace object then shows the new names.
fn run_include(it: &mut Interp, path: PathBuf, decls: &Scope) -> R<(Include, Option<Value>)> {
    if std::fs::File::open(&path).is_err() {
        return Ok((Include::NetworkError, None));
    }
    let script = decls.borrow().script.clone();
    let Some(ctx) = script.as_ref().and_then(|s| s.ctx.as_ref()) else {
        unreachable!("calling_script returns the scope of a file context")
    };
    let aot = ctx.aot.clone();
    let code = Interp::child(decls);
    code.borrow_mut().script = Some(Rc::new(ScriptFile {
        file: path.clone(),
        ctx: None,
    }));
    let global = std::mem::replace(&mut it.global, decls.clone());
    let r = match aot.as_ref().and_then(|a| a.script(&path)) {
        Some(e) => crate::jsrt::run_entry(
            it,
            e,
            &crate::jsrt::Env::from_scope(code),
            Value::Undefined,
            &[],
        ),
        #[cfg(feature = "interp")]
        None => match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|src| crate::ir::lower_program(&src))
        {
            Ok(body) => it.run_included(&body, decls, &code),
            Err(m) => it.throw("SyntaxError", &m),
        },
        #[cfg(not(feature = "interp"))]
        None => it.throw("Error", format!("{}: no compiled code", path.display())),
    };
    it.global = global;
    ctx.export_vars(decls);
    match r {
        Ok(_) => Ok((Include::Ok, None)),
        Err(Ex::Throw(e)) => Ok((Include::Exception, Some(e))),
        Err(e) => Err(e),
    }
}

/// `Qt.fontFamilies()`: QFontDatabase::families, the primary family name of every font
/// fontconfig lists, sorted.
pub fn font_families(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    arity(it, a, 0)?;
    Ok(Value::array(
        system_font_families()
            .into_iter()
            .map(|s| Value::str(&s))
            .collect(),
    ))
}

/// FcWeightFromOpenTypeDouble: fontconfig's weight for an OpenType one, between the points
/// of its table.
fn fc_weight(weight: f64) -> std::ffi::c_int {
    const MAP: [(f64, f64); 12] = [
        (0.0, 0.0),
        (100.0, 0.0),
        (200.0, 40.0),
        (300.0, 50.0),
        (350.0, 55.0),
        (380.0, 75.0),
        (400.0, 80.0),
        (500.0, 100.0),
        (600.0, 180.0),
        (700.0, 200.0),
        (800.0, 205.0),
        (900.0, 210.0),
    ];
    let w = weight.clamp(0.0, 900.0);
    let i = MAP.iter().position(|p| p.0 >= w).unwrap_or(11).max(1);
    let ((x0, y0), (x1, y1)) = (MAP[i - 1], MAP[i]);
    (y0 + (w - x0) * (y1 - y0) / (x1 - x0)).round() as std::ffi::c_int
}

/// The family and style of the font fontconfig picks for `family` at OpenType `weight`,
/// italic or not: what QFontInfo of such a font says.
pub(crate) fn font_match(family: &str, weight: f64, italic: bool) -> Option<[String; 2]> {
    use fontconfig_sys as fc;
    use fontconfig_sys::constants::{FC_FAMILY, FC_SLANT, FC_STYLE, FC_WEIGHT};
    let name = std::ffi::CString::new(family).ok()?;
    // SAFETY: creates an empty pattern.
    let pat = unsafe { fc::FcPatternCreate() };
    // SAFETY: our pattern; the name is a static NUL-terminated string and fontconfig copies
    // `name`, which lives past the call.
    unsafe { fc::FcPatternAddString(pat, FC_FAMILY.as_ptr(), name.as_ptr().cast()) };
    // SAFETY: our pattern and a static NUL-terminated name.
    unsafe { fc::FcPatternAddInteger(pat, FC_WEIGHT.as_ptr(), fc_weight(weight)) };
    // SAFETY: our pattern and a static NUL-terminated name.
    unsafe { fc::FcPatternAddInteger(pat, FC_SLANT.as_ptr(), if italic { 100 } else { 0 }) };
    // What FcFontMatch asks for first, on fontconfig's current configuration (NULL).
    // SAFETY: our pattern; NULL names the current configuration.
    unsafe { fc::FcConfigSubstitute(std::ptr::null_mut(), pat, fc::FcMatchPattern) };
    // SAFETY: our pattern.
    unsafe { fc::FcDefaultSubstitute(pat) };
    let mut result = fc::FcResultNoMatch;
    // SAFETY: a read-only query with our pattern on the current configuration.
    let found = unsafe { fc::FcFontMatch(std::ptr::null_mut(), pat, &mut result) };
    // SAFETY: ours, destroyed once.
    unsafe { fc::FcPatternDestroy(pat) };
    if found.is_null() {
        return None;
    }
    let text = |key: &std::ffi::CStr| {
        let mut v: *mut fc::FcChar8 = std::ptr::null_mut();
        // SAFETY: `found` is a live pattern; the string stays owned by it.
        let r = unsafe { fc::FcPatternGetString(found, key.as_ptr(), 0, &mut v) };
        (r == fc::FcResultMatch && !v.is_null()).then(|| {
            // SAFETY: fontconfig returns a NUL-terminated UTF-8 string owned by `found`.
            let s = unsafe { std::ffi::CStr::from_ptr(v as *const std::ffi::c_char) };
            s.to_string_lossy().into_owned()
        })
    };
    let out = text(FC_FAMILY).zip(text(FC_STYLE)).map(|(f, s)| [f, s]);
    // SAFETY: ours, destroyed once; the copied names do not borrow from it.
    unsafe { fc::FcPatternDestroy(found) };
    out
}

pub(crate) fn system_font_families() -> Vec<String> {
    use fontconfig_sys as fc;
    use fontconfig_sys::constants::FC_FAMILY;
    // SAFETY: creates an empty pattern (matches every font).
    let pat = unsafe { fc::FcPatternCreate() };
    // SAFETY: creates an empty object set.
    let os = unsafe { fc::FcObjectSetCreate() };
    // SAFETY: our object set; FC_FAMILY is a static NUL-terminated name.
    unsafe { fc::FcObjectSetAdd(os, FC_FAMILY.as_ptr()) };
    // SAFETY: a read-only query on fontconfig's current configuration (NULL), which
    // fontconfig initialises on first use.
    let set = unsafe { fc::FcFontList(std::ptr::null_mut(), pat, os) };
    // SAFETY: ours, destroyed once.
    unsafe { fc::FcObjectSetDestroy(os) };
    // SAFETY: ours, destroyed once.
    unsafe { fc::FcPatternDestroy(pat) };
    let mut out = Vec::new();
    if set.is_null() {
        return out;
    }
    // SAFETY: `set` is a live FcFontSet.
    let s = unsafe { &*set };
    let fonts = if s.fonts.is_null() {
        &[][..]
    } else {
        // SAFETY: `fonts` holds `nfont` patterns, alive until FcFontSetDestroy below.
        unsafe { std::slice::from_raw_parts(s.fonts, usize::try_from(s.nfont).unwrap_or(0)) }
    };
    for &p in fonts {
        let mut v: *mut fc::FcChar8 = std::ptr::null_mut();
        // SAFETY: `p` belongs to the live `set`; the string stays owned by the pattern.
        let r = unsafe { fc::FcPatternGetString(p, FC_FAMILY.as_ptr(), 0, &mut v) };
        if r == fc::FcResultMatch && !v.is_null() {
            // SAFETY: fontconfig returns a NUL-terminated UTF-8 string owned by `p`.
            let s = unsafe { std::ffi::CStr::from_ptr(v as *const std::ffi::c_char) };
            out.push(s.to_string_lossy().into_owned());
        }
    }
    // SAFETY: ours, destroyed once; the copied names do not borrow from it.
    unsafe { fc::FcFontSetDestroy(set) };
    out.sort();
    out.dedup();
    out
}

#[cfg(all(test, feature = "interp"))]
mod tests {
    use crate::runtime::Runtime;
    use crate::value::Value;

    /// Runs `body` (a JS function body that pushes to `out`) in a QML document and returns
    /// what it pushed, each value or thrown error as `JSON.stringify` / `ERR name: message`.
    fn run(body: &str) -> Vec<String> {
        let mut rt = Runtime::new(None);
        let src = format!(
            "import QtQuick\nItem {{\n property var out: []\n Component.onCompleted: {{\n \
             const r = [];\n function t(f) {{ try {{ r.push(JSON.stringify(f())) }} catch (e) \
             {{ r.push('ERR ' + e.name + ': ' + e.message) }} }}\n {body}\n out = r\n }}\n}}"
        );
        let root = rt.load_str(&src).unwrap();
        rt.settle();
        let Value::Obj(o) = rt.get(root, "out") else {
            panic!("no output: {:?}", rt.errors())
        };
        let a = o.borrow().array.clone().unwrap_or_default();
        a.iter().map(|v| v.to_js_string()).collect()
    }

    // Expected values are Qt 6.12's (PySide6 6.12, QQmlApplicationEngine, offscreen).

    /// `Qt.include` from a `.js` import, at load and from a later call out of QML: the included
    /// files run in the import's context (its namespace shows their declarations) and a file
    /// they include resolves against them.
    #[test]
    fn include_runs_files_in_the_calling_file_context() {
        let dir = tempfile::tempdir().unwrap();
        let w = |p: &str, s: &str| {
            let f = dir.path().join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, s).unwrap();
        };
        w(
            "lib.js",
            "var r = Qt.include(\"sub/a.js\");\n\
             function late() { return Qt.include(\"sub/late.js\").status }\n",
        );
        w("sub/a.js", "var a = 1;\nQt.include(\"b.js\");\n");
        w("sub/b.js", "var b = a + 1;\n");
        w("sub/late.js", "var lateVar = 'l';\n");
        w(
            "Main.qml",
            "import QtQuick\nimport \"lib.js\" as L\nItem {\n property var out: []\n \
             Component.onCompleted: out = [L.r.status, L.a, L.b, L.late(), L.lateVar]\n}\n",
        );
        let mut rt = Runtime::new(None);
        let root = rt.load_file(&dir.path().join("Main.qml")).unwrap();
        rt.settle();
        let Value::Obj(o) = rt.get(root, "out") else {
            panic!("no output: {:?}", rt.errors())
        };
        let a = o.borrow().array.clone().unwrap_or_default();
        let out: Vec<String> = a.iter().map(|v| v.to_js_string()).collect();
        assert_eq!(out, ["0", "1", "2", "0", "l"], "{:?}", rt.errors());
    }

    #[test]
    fn qt_members_enumerate_in_declaration_order() {
        let out = run("t(() => Object.keys(Qt).filter(k => typeof Qt[k] !== 'number').join())");
        let keys = out[0].trim_matches('"').split(',').collect::<Vec<_>>();
        let qt: Vec<&str> = crate::builtins::QT_MEMBERS
            .iter()
            .copied()
            .filter(|k| keys.contains(k))
            .collect();
        assert_eq!(keys, qt);
        for k in [
            "objectName",
            "application",
            "inputMethod",
            "styleHints",
            "uiLanguage",
            "objectNameChanged",
            "uiLanguageChanged",
            "include",
            "isQtObject",
            "colorEqual",
            "quaternion",
            "matrix4x4",
            "tint",
            "url",
            "font",
            "fontFamilies",
            "btoa",
            "atob",
            "escapeHtml",
            "createComponent",
            "enumStringToValue",
            "enumValueToString",
            "enumValueToStrings",
        ] {
            assert!(keys.contains(&k), "{k} missing from {keys:?}");
        }
    }

    #[test]
    fn color_equal_and_tint() {
        let out = run(r##"
            t(() => Qt.colorEqual("red", "#ff0000"))
            t(() => Qt.colorEqual(Qt.rgba(1, 0, 0, 0.5), "#80ff0000"))
            t(() => Qt.colorEqual(Qt.hsla(0, 1, 0.5, 1), "red"))
            t(() => Qt.colorEqual("red"))
            t(() => Qt.colorEqual("red", "nope"))
            t(() => Qt.colorEqual(1, "red"))
            t(() => String(Qt.tint("red", "#800000ff")))
            t(() => String(Qt.tint("#ff0000", "#0000ff")))
            t(() => String(Qt.tint("#80ff0000", "#4000ff00")))
            t(() => String(Qt.tint("red", "transparent")))
            t(() => Qt.tint(1, 2))
            t(() => Qt.tint("red"))
        "##);
        assert_eq!(
            out,
            [
                "true",
                "false",
                "false",
                "ERR Error: Insufficient arguments",
                "ERR Error: Qt.colorEqual(): Invalid color name",
                "ERR Error: Qt.colorEqual(): Invalid arguments",
                "\"#7f0080\"",
                "\"#0000ff\"",
                "\"#a0bf4000\"",
                "\"#ff0000\"",
                "null",
                "ERR Error: Insufficient arguments",
            ]
        );
    }

    #[test]
    fn quaternion_and_matrix4x4() {
        let out = run(r#"
            t(() => Qt.quaternion(1, 2, 3, 4))
            t(() => Qt.quaternion(1, 2, 3))
            t(() => Object.keys(Qt.matrix4x4()).map(k => Qt.matrix4x4()[k]).join())
            t(() => Qt.matrix4x4(1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16).m23)
            t(() => Qt.matrix4x4([1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16]).m41)
            t(() => Qt.matrix4x4([1, 2]))
            t(() => Qt.matrix4x4(1, 2, 3, 4, 5))
        "#);
        assert_eq!(
            out,
            [
                r#"{"scalar":1,"x":2,"y":3,"z":4}"#,
                "ERR Error: Insufficient arguments",
                "\"1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1\"",
                "7",
                "13",
                "ERR Error: Qt.matrix4x4(): Invalid argument: not a valid matrix4x4 values array",
                "ERR Error: Too many arguments",
            ]
        );
    }

    #[test]
    fn text_helpers() {
        let out = run(r#"
            t(() => Qt.escapeHtml(`<a href="x">'&'</a>`))
            t(() => Qt.escapeHtml(5))
            t(() => Qt.escapeHtml())
            t(() => Qt.btoa("hello"))
            t(() => Qt.btoa("ä"))
            t(() => Qt.atob("w6Q="))
            t(() => Qt.atob("aGVsbG8"))
            t(() => Qt.atob("aGVs bG8="))
            t(() => Qt.atob("aGVsbG8=junk").length)
            t(() => Qt.atob("a"))
            t(() => Qt.atob("aG"))
            t(() => Qt.atob("-_8="))
            t(() => Qt.btoa(null))
            t(() => String(Qt.url("http://a/%7Ex")))
            t(() => Qt.url(1))
            t(() => Qt.url())
            t(() => Qt.url("a", "b"))
        "#);
        assert_eq!(
            out,
            [
                r#""&lt;a href=&quot;x&quot;&gt;'&amp;'&lt;/a&gt;""#,
                "\"5\"",
                "ERR Error: Insufficient arguments",
                "\"aGVsbG8=\"",
                "\"w6Q=\"",
                "\"ä\"",
                "\"hello\"",
                "\"hello\"",
                "8",
                "\"\"",
                "\"h\"",
                "\"\"",
                "ERR TypeError: Passing incompatible arguments to C++ functions from JavaScript is not allowed.",
                "\"http://a/~x\"",
                "ERR TypeError: Passing incompatible arguments to C++ functions from JavaScript is not allowed.",
                "ERR Error: Insufficient arguments",
                "ERR Error: Too many arguments",
            ]
        );
    }

    #[test]
    fn binary_base64_overloads() {
        let out = run(r#"
            const b = v => v instanceof ArrayBuffer ? "AB " + Array.from(new Uint8Array(v)) : v;
            const u = (...x) => new Uint8Array(x).buffer;
            t(() => b(Qt.btoa(u(104, 105))))
            t(() => b(Qt.btoa(u())))
            t(() => b(Qt.btoa([104, "i", 105.9])))
            t(() => b(Qt.btoa([NaN])))
            t(() => b(Qt.btoa([1], undefined)))
            t(() => b(Qt.btoa(5)))
            t(() => Qt.btoa([256]))
            t(() => Qt.btoa([-1]))
            t(() => Qt.btoa(["ab"]))
            t(() => Qt.btoa(["\u0100"]))
            t(() => Qt.btoa([true]))
            t(() => Qt.btoa([1, , 2]))
            t(() => Qt.btoa(Symbol("x")))
            t(() => Qt.btoa([Symbol("x")]))
            t(() => b(Qt.atob(u(97, 71, 107, 61))))
            t(() => b(Qt.atob(["a", "G", "k"])))
            t(() => b(Qt.atob(["a"])))
            t(() => Qt.atob([97, 32, 71, 107]))
            t(() => Qt.atob(["a", "="]))
            t(() => Qt.atob(["a", "=", "=", "="]))
            t(() => Qt.atob(["a", "G", "=", "x"]))
            t(() => Qt.atob(["-", "_"]))
            t(() => { const x = u(1); return Qt.atob(Qt.btoa(x)) !== x })
            t(() => { try { Qt.atob(["!"]) } catch (e) {
                return [Object.getPrototypeOf(e) === Error.prototype, e.code, Object.keys(e)] } })
            t(() => Qt.btoa(/a/))
            t(() => Qt.btoa(new URL("http://a/")))
            t(() => Qt.btoa(Qt.rgba(0, 0, 0, 1)))
            t(() => Qt.btoa())
            t(() => Qt.btoa("a", "b"))
            t(() => typeof Qt.btoa(new Date(0)))
            t(() => Qt.atob("77u/QQ=="))
            t(() => Qt.atob("4oJB"))
            t(() => Qt.atob("7aCAQQ=="))
        "#);
        let incompatible = "ERR TypeError: Passing incompatible arguments to C++ functions from JavaScript is not allowed.";
        let invalid = "ERR Error: Invalid character";
        assert_eq!(
            out,
            [
                "\"AB 97,71,107,61\"",
                "\"AB \"",
                "\"AB 97,71,108,112\"",
                "\"AB 65,65,61,61\"",
                "\"AB 65,81,61,61\"",
                "\"AB \"",
                invalid,
                invalid,
                invalid,
                invalid,
                invalid,
                invalid,
                invalid,
                invalid,
                "\"AB 104,105\"",
                "\"AB 104,105\"",
                "\"AB \"",
                invalid,
                invalid,
                invalid,
                invalid,
                invalid,
                "true",
                "[true,5,[\"code\"]]",
                incompatible,
                incompatible,
                incompatible,
                "ERR Error: Unable to determine callable overload.  Candidates are:\n    btoa(QVariantList)\n    btoa(QByteArray)\n    btoa(QString)",
                "ERR Error: Too many arguments",
                "\"string\"",
                "\"A\"",
                "\"\u{FFFD}\u{FFFD}A\"",
                "\"\u{FFFD}\u{FFFD}\u{FFFD}A\"",
            ]
        );
    }

    #[test]
    fn font_specifier() {
        let out = run(r#"
            function d(f) { return Object.keys(f).map(k => k + "=" + JSON.stringify(f[k])).join(" ") }
            t(() => d(Qt.font({family: "X", pointSize: 12, bold: true, italic: true})))
            t(() => [Qt.font({pixelSize: 20, weight: 300}).pointSize, Qt.font({pixelSize: 20, weight: 300}).weight])
            t(() => Qt.font({weight: "x", family: "Y"}).weight)
            t(() => Qt.font({family: "Z", underline: 1}).underline)
            t(() => Qt.font({}))
            t(() => Qt.font({pixelSize: "14"}))
            t(() => Qt.font("Arial"))
            t(() => Qt.font())
        "#);
        assert_eq!(
            out,
            [
                "\"family=\\\"X\\\" styleName=\\\"\\\" bold=true weight=700 italic=true \
                 underline=false overline=false strikeout=false pointSize=12 pixelSize=16 \
                 capitalization=0 letterSpacing=0 wordSpacing=0 hintingPreference=0 \
                 kerning=true preferShaping=true features={} variableAxes={} \
                 contextFontMerging=false preferTypoLineMetrics=false\"",
                "[15,300]",
                "400",
                "false",
                "ERR Error: Qt.font(): Invalid argument: no valid font subproperties specified",
                "ERR Error: Qt.font(): Invalid argument: no valid font subproperties specified",
                "ERR Error: Qt.font(): Invalid arguments",
                "ERR Error: Insufficient arguments",
            ]
        );
    }

    #[test]
    fn enum_helpers() {
        let out = run(r#"
            t(() => Qt.enumStringToValue(Text.HAlignment, "AlignRight"))
            t(() => Qt.enumStringToValue(Text.HAlignment, {toString() { return "AlignLeft" }}))
            t(() => Qt.enumStringToValue(Text.HAlignment, "Nope"))
            t(() => Qt.enumValueToString(Text.HAlignment, 1.5))
            t(() => Qt.enumValueToString(Text.HAlignment, 1.4))
            t(() => Qt.enumValueToString(Text.HAlignment, "1"))
            t(() => Qt.enumValueToString(Text.HAlignment, NaN))
            t(() => Qt.enumValueToString(Text.HAlignment, 2.5))
            t(() => Qt.enumValueToStrings(Text.HAlignment, 4))
            t(() => Qt.enumValueToString(Text, 1))
            t(() => Qt.enumValueToString({AlignLeft: 1}, 1))
            t(() => Qt.enumValueToString(Text.HAlignment))
            t(() => Qt.enumStringToValue(Text.HAlignment, "AlignLeft", 3))
        "#);
        assert_eq!(
            out,
            [
                "2",
                "1",
                "ERR ReferenceError: Invalid second argument, entry is not defined",
                "\"AlignRight\"",
                "\"AlignLeft\"",
                "\"AlignLeft\"",
                "ERR ReferenceError: Invalid second argument, entry is not defined",
                "ERR ReferenceError: Invalid second argument, entry is not defined",
                "[\"AlignHCenter\"]",
                "ERR TypeError: Invalid first argument, expected enum",
                "ERR TypeError: Invalid first argument, expected enum",
                "ERR Error: Insufficient arguments",
                "ERR Error: Too many arguments",
            ]
        );
    }

    #[test]
    fn duplicate_enum_values_list_every_key_in_declaration_order() {
        // Qt.Key: Key_Space and Key_Any are both 32; Qt names Key_Any and lists both.
        let e = crate::types::QmlEnum(vec![
            ("Key_Space".into(), 32.0),
            ("Key_Any".into(), 32.0),
            ("Key_Exclam".into(), 33.0),
        ]);
        let keys = super::enum_keys(&e, Some(32.0));
        assert_eq!(
            keys.iter().map(|k| &**k).collect::<Vec<_>>(),
            ["Key_Space", "Key_Any"]
        );
        assert!(super::enum_keys(&e, None).is_empty());
    }

    #[test]
    fn qobject_members() {
        let out = run(r#"
            let seen = []
            Qt.objectNameChanged.connect(() => seen.push("o:" + Qt.objectName))
            Qt.uiLanguageChanged.connect(() => seen.push("u:" + Qt.uiLanguage))
            t(() => Qt.objectName)
            Qt.objectName = "nm"
            Qt.objectName = "nm"
            Qt.uiLanguage = 5
            t(() => [Qt.objectName, Qt.uiLanguage])
            t(() => seen)
            t(() => { Qt.objectName = undefined })
            t(() => [Qt.isQtObject(Qt), Qt.isQtObject(Qt.application), Qt.isQtObject({}),
                     Qt.isQtObject(null), Qt.isQtObject("red"), Qt.isQtObject(Qt.objectNameChanged)])
            t(() => Qt.isQtObject())
            t(() => Qt.application.styleHints === Qt.styleHints)
            t(() => [Qt.styleHints.mouseDoubleClickInterval, Qt.styleHints.startDragDistance,
                     Qt.styleHints.wheelScrollLines, Qt.styleHints.cursorFlashTime,
                     Qt.styleHints.mousePressAndHoldInterval, Qt.styleHints.passwordMaskCharacter])
            t(() => [Qt.application.state, Qt.application.supportsMultipleWindows,
                     Qt.application.displayName === Qt.application.name])
            t(() => [Qt.inputMethod.visible, typeof Qt.inputMethod.locale.name])
            t(() => { Qt.styleHints = null })
            t(() => [typeof gc, gc(), gc(1, 2), gc.length, gc.name])
            t(() => Qt.include("x.js"))
            t(() => Array.isArray(Qt.fontFamilies()))
            t(() => Qt.fontFamilies(1))
        "#);
        assert_eq!(
            out,
            [
                "\"\"",
                "[\"nm\",\"5\"]",
                "[\"o:nm\",\"u:5\"]",
                "ERR Error: Cannot assign [undefined] to QString",
                "[true,true,false,false,false,false]",
                "ERR Error: Insufficient arguments",
                "true",
                "[400,10,3,1000,800,\"\u{25cf}\"]",
                "[2,true,true]",
                "[false,\"string\"]",
                "ERR TypeError: Cannot assign to read-only property \"styleHints\"",
                "[\"function\",null,null,0,\"gc\"]",
                "ERR Error: Qt.include(): Can only be called from JavaScript files",
                "true",
                "ERR Error: Too many arguments",
            ]
        );
    }

    #[test]
    fn input_method_without_a_focused_editor() {
        let out = run(r#"
            const im = Qt.inputMethod
            const rect = r => [r.x, r.y, r.width, r.height].join()
            t(() => [im === Qt.inputMethod, im.visible, im.animating, im.inputDirection,
                     typeof im.locale.name])
            t(() => [rect(im.cursorRectangle), rect(im.anchorRectangle),
                     rect(im.keyboardRectangle), rect(im.inputItemClipRectangle)])
            t(() => [typeof im.show, typeof im.update, typeof im.invokeAction, typeof im.setVisible])
            t(() => [im.show(), im.hide(), im.reset(), im.commit(), im.update(Qt.ImQueryAll),
                     im.invokeAction(InputMethod.Click, 1), im.show(1), im.visible])
            t(() => im.update())
            t(() => im.invokeAction(0))
            t(() => [InputMethod.Click, InputMethod.ContextMenu])
        "#);
        assert_eq!(
            out,
            [
                "[true,false,false,0,\"string\"]",
                "[\"0,0,0,0\",\"0,0,0,0\",\"0,0,0,0\",\"0,0,0,0\"]",
                "[\"function\",\"function\",\"function\",\"undefined\"]",
                "[null,null,null,null,null,null,null,false]",
                "ERR Error: Insufficient arguments",
                "ERR Error: Insufficient arguments",
                "[0,1]",
            ]
        );
    }

    #[test]
    fn ui_language_is_the_bcp47_name_of_the_system_locale() {
        assert_eq!(super::bcp47_name("C"), "en-POSIX");
        assert_eq!(super::bcp47_name("en_US"), "en");
        assert_eq!(super::bcp47_name("en_GB"), "en-GB");
        assert_eq!(super::bcp47_name("pt_BR"), "pt");
    }

    #[test]
    fn create_component() {
        let dir = std::env::temp_dir().join(format!("qtobj-cc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Dyn.qml"),
            "import QtQuick\nItem { property int n: 7 }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("main.qml"),
            r#"import QtQuick
Item {
    id: root
    property var made: null
    property var info: []
    Component.onCompleted: {
        const c = Qt.createComponent("Dyn.qml", root)
        made = c.createObject(root)
        const bad = Qt.createComponent("Nope.qml", Component.PreferSynchronous, root)
        const ty = Qt.createComponent("QtQuick", "Rectangle", root)
        const nt = Qt.createComponent("QtQuick", "Nope")
        let err = ""
        try { Qt.createComponent(1) } catch (e) { err = e.name + ": " + e.message }
        info = [c.status, c.errorString(), bad.status, bad.errorString().endsWith("Nope.qml: No such file or directory\n"),
                bad.createObject(root), ty.status, ty.createObject(root) !== null, nt.status, nt.errorString(), err,
                Qt.isQtObject(c)]
    }
}
"#,
        )
        .unwrap();
        let mut rt = Runtime::new(None);
        let root = rt.load_file(&dir.join("main.qml")).unwrap();
        rt.settle();
        let Value::Item(m) = rt.get(root, "made") else {
            panic!("no object: {:?}", rt.errors())
        };
        assert_eq!(rt.get(m, "n").to_number(), 7.0);
        let Value::Obj(o) = rt.get(root, "info") else {
            panic!()
        };
        let info: Vec<String> = o
            .borrow()
            .array
            .clone()
            .unwrap()
            .iter()
            .map(|v| v.to_js_string())
            .collect();
        assert_eq!(
            info,
            [
                "1",
                "",
                "3",
                "true",
                "null",
                "1",
                "true",
                "3",
                "<Unknown File>: Module \"QtQuick\" contains no type named \"Nope\"\n",
                "TypeError: Passing incompatible arguments to C++ functions from JavaScript is not allowed.",
                "true",
            ]
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
