//! `ArrayBuffer`, the typed arrays and `DataView` (ECMA-262 §25.1, §23.2, §25.3), over a
//! shared byte store: a typed array or `DataView` is a window (`View`) onto its buffer's bytes.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;

use crate::builtins::native;
use crate::eval::{
    Callable, Interp, NativeFn, R, callable, define_accessor, fn_value, link_builtin_prototype,
    new_obj, set_hidden,
};
use crate::symbol::species_constructor;
use crate::value::{Internal, Obj, Value, num_to_string, str_to_number};

/// An `ArrayBuffer`'s [[ArrayBufferData]].
pub type Bytes = Rc<RefCell<Vec<u8>>>;

/// Largest `ArrayBuffer` (as V4, whose buffers are `QByteArray`s indexed by `int`).
const MAX_BYTES: f64 = i32::MAX as f64;

/// A typed array's [[TypedArrayName]] (ECMA-262 Table 71).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    I8,
    U8,
    U8C,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

const KINDS: [Kind; 9] = [
    Kind::I8,
    Kind::U8,
    Kind::U8C,
    Kind::I16,
    Kind::U16,
    Kind::I32,
    Kind::U32,
    Kind::F32,
    Kind::F64,
];

/// The constructor names the global object gets, for [`crate::builtins::GLOBALS`].
pub const NAMES: [&str; 13] = [
    "ArrayBuffer",
    "SharedArrayBuffer",
    "DataView",
    "Atomics",
    "Int8Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "Int16Array",
    "Uint16Array",
    "Int32Array",
    "Uint32Array",
    "Float32Array",
    "Float64Array",
];

/// ToIntegerOrInfinity's modular step for ToInt8 .. ToUint32 (ECMA-262 §7.1.6-11).
fn modular(v: f64, bits: i32) -> u64 {
    if !v.is_finite() {
        return 0;
    }
    v.trunc().rem_euclid(2f64.powi(bits)) as u64
}

impl Kind {
    pub fn name(self) -> &'static str {
        NAMES[4 + self as usize]
    }

    pub fn size(self) -> usize {
        match self {
            Kind::I8 | Kind::U8 | Kind::U8C => 1,
            Kind::I16 | Kind::U16 => 2,
            Kind::I32 | Kind::U32 | Kind::F32 => 4,
            Kind::F64 => 8,
        }
    }

    fn from_name(n: &str) -> Option<Kind> {
        KINDS.into_iter().find(|k| k.name() == n)
    }

    /// RawBytesToNumeric (ECMA-262 §25.1.3.15) of `b`, `size()` bytes.
    pub fn decode(self, b: &[u8], little: bool) -> f64 {
        macro_rules! de {
            ($t:ty) => {{
                let a = b.try_into().unwrap();
                (if little {
                    <$t>::from_le_bytes(a)
                } else {
                    <$t>::from_be_bytes(a)
                }) as f64
            }};
        }
        match self {
            Kind::I8 => b[0] as i8 as f64,
            Kind::U8 | Kind::U8C => b[0] as f64,
            Kind::I16 => de!(i16),
            Kind::U16 => de!(u16),
            Kind::I32 => de!(i32),
            Kind::U32 => de!(u32),
            Kind::F32 => de!(f32),
            Kind::F64 => de!(f64),
        }
    }

    /// NumericToRawBytes (ECMA-262 §25.1.3.17) of number `v` into `b`, `size()` bytes.
    pub fn encode(self, b: &mut [u8], v: f64, little: bool) {
        macro_rules! en {
            ($x:expr) => {{
                let x = $x;
                b.copy_from_slice(&if little {
                    x.to_le_bytes()
                } else {
                    x.to_be_bytes()
                });
            }};
        }
        match self {
            Kind::I8 | Kind::U8 => b[0] = modular(v, 8) as u8,
            Kind::U8C => {
                b[0] = if v.is_nan() {
                    0
                } else {
                    v.clamp(0.0, 255.0).round_ties_even() as u8
                }
            }
            Kind::I16 | Kind::U16 => en!(modular(v, 16) as u16),
            Kind::I32 | Kind::U32 => en!(modular(v, 32) as u32),
            Kind::F32 => en!(v as f32),
            Kind::F64 => en!(v),
        }
    }
}

/// A typed array's or `DataView`'s window on its buffer: [[ViewedArrayBuffer]],
/// [[ByteOffset]] and [[ArrayLength]] (elements of `kind`; bytes for a `DataView`).
#[derive(Debug)]
pub struct View {
    pub kind: Kind,
    pub buffer: Obj,
    pub data: Bytes,
    pub offset: usize,
    pub len: usize,
}

impl View {
    fn byte_len(&self) -> usize {
        self.len * self.kind.size()
    }

    /// Element `i`, `None` past the end.
    pub fn read(&self, i: usize) -> Option<f64> {
        if i >= self.len {
            return None;
        }
        let s = self.kind.size();
        let at = self.offset + i * s;
        Some(self.kind.decode(
            &self.data.borrow()[at..at + s],
            cfg!(target_endian = "little"),
        ))
    }

    /// Sets element `i` to number `v`; nothing past the end.
    pub fn write(&self, i: usize, v: f64) {
        if i < self.len {
            let s = self.kind.size();
            let at = self.offset + i * s;
            let b = &mut self.data.borrow_mut()[at..at + s];
            self.kind.encode(b, v, cfg!(target_endian = "little"));
        }
    }

    pub fn numbers(&self) -> Vec<f64> {
        (0..self.len).filter_map(|i| self.read(i)).collect()
    }

    pub fn values(&self) -> Vec<Value> {
        self.numbers().into_iter().map(Value::Num).collect()
    }
}

/// CanonicalNumericIndexString (ECMA-262 §7.1.21): the number key `k` names, if any.
pub fn numeric_key(k: &str) -> Option<f64> {
    let c = *k.as_bytes().first()?;
    if !(c.is_ascii_digit() || c == b'-' || c == b'I' || c == b'N') {
        return None;
    }
    if k == "-0" {
        return Some(-0.0);
    }
    let n = str_to_number(k);
    (num_to_string(n) == k).then_some(n)
}

/// IsValidIntegerIndex (ECMA-262 §10.4.5.14).
fn valid_index(v: &View, n: f64) -> Option<usize> {
    (n.fract() == 0.0 && !(n == 0.0 && n.is_sign_negative()) && n >= 0.0 && n < v.len as f64)
        .then_some(n as usize)
}

/// The typed array view of `o`, if it is one.
pub fn typed(o: &Obj) -> Option<std::cell::Ref<'_, View>> {
    std::cell::Ref::filter_map(o.borrow(), |b| match &b.internal {
        Internal::TypedArray(v) => Some(&**v),
        _ => None,
    })
    .ok()
}

/// [[Get]] of a typed array's numeric key `k` (ECMA-262 §10.4.5.4): `None` for any other key,
/// which takes the ordinary path.
pub fn get_index(v: &View, k: &str) -> Option<Value> {
    let n = numeric_key(k)?;
    Some(
        valid_index(v, n)
            .and_then(|i| v.read(i))
            .map_or(Value::Undefined, Value::Num),
    )
}

/// [[Set]] of a typed array's numeric key `k` (ECMA-262 §10.4.5.5, TypedArraySetElement):
/// `false` for any other key. The value is converted first, even for an index out of range.
pub fn put_index(it: &mut Interp, o: &Obj, k: &str, val: &Value) -> R<bool> {
    let Some(n) = numeric_key(k) else {
        return Ok(false);
    };
    let x = it.to_numeric(val)?;
    if let Some(v) = typed(o)
        && let Some(i) = valid_index(&v, n)
    {
        v.write(i, x);
    }
    Ok(true)
}

/// [[GetOwnProperty]] of a typed array's numeric key `k` (ECMA-262 §10.4.5.1): the element,
/// `Some(None)` for an invalid index, `None` for any other key.
pub fn own_index(v: &View, k: &str) -> Option<Option<Value>> {
    let n = numeric_key(k)?;
    Some(valid_index(v, n).and_then(|i| v.read(i)).map(Value::Num))
}

/// [[HasProperty]]/[[GetOwnProperty]] of numeric key `k`: `None` for any other key.
pub fn has_index(v: &View, k: &str) -> Option<bool> {
    numeric_key(k).map(|n| valid_index(v, n).is_some())
}

fn arg(a: &[Value], i: usize) -> Value {
    a.get(i).cloned().unwrap_or_default()
}

fn obj(o: Obj) -> Value {
    Value::Obj(o)
}

const BAD_OFFSET: &str = "new TypedArray: invalid byteOffset";
const BAD_LENGTH: &str = "new TypedArray: invalid length";
const SET_RANGE: &str = "TypedArray.set: out of range";
const VIEW_RANGE: &str = "DataView: constructor arguments out of range";
const OUT_OF_RANGE: &str = "index out of range";

/// ToIndex (ECMA-262 §7.1.22), RangeError `what` when out of range.
fn to_index(it: &mut Interp, v: &Value, what: &str) -> R<usize> {
    if matches!(v, Value::Undefined) {
        return Ok(0);
    }
    let n = integer(it, v)?;
    if !(0.0..=MAX_BYTES).contains(&n) {
        return it.throw("RangeError", what.to_string());
    }
    Ok(n as usize)
}

/// ToIntegerOrInfinity (ECMA-262 §7.1.5).
fn integer(it: &mut Interp, v: &Value) -> R<f64> {
    let n = it.to_numeric(v)?;
    Ok(if n.is_nan() { 0.0 } else { n.trunc() + 0.0 })
}

/// A relative index argument (`start`, `end`, ...) resolved against `len`, `dflt` when undefined.
fn relative(it: &mut Interp, v: &Value, len: usize, dflt: usize) -> R<usize> {
    if matches!(v, Value::Undefined) {
        return Ok(dflt);
    }
    let n = integer(it, v)?;
    let len = len as f64;
    Ok(if n < 0.0 {
        (len + n).max(0.0)
    } else {
        n.min(len)
    } as usize)
}

/// The `prototype` of global constructor `name` (the intrinsic, as global constructors
/// cannot be reassigned from QML).
fn intrinsic_proto(it: &Interp, name: &str) -> Option<Obj> {
    match it.global.borrow().vars.get(name) {
        Some(Value::Obj(f)) => match f.borrow().props.get("prototype") {
            Some(Value::Obj(p)) => Some(p.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn with_internal(it: &Interp, proto: &str, internal: Internal) -> Obj {
    let o = new_obj();
    {
        let mut b = o.borrow_mut();
        b.proto = intrinsic_proto(it, proto);
        b.internal = internal;
    }
    o
}

/// AllocateArrayBuffer (ECMA-262 §25.1.3.1) holding `bytes`.
pub(crate) fn new_buffer(it: &Interp, bytes: Vec<u8>) -> (Obj, Bytes) {
    let data = Rc::new(RefCell::new(bytes));
    (
        with_internal(it, "ArrayBuffer", Internal::ArrayBuffer(data.clone())),
        data,
    )
}

pub(crate) fn new_view(
    it: &Interp,
    kind: Kind,
    buffer: Obj,
    data: Bytes,
    offset: usize,
    len: usize,
) -> Obj {
    let v = View {
        kind,
        buffer,
        data,
        offset,
        len,
    };
    with_internal(it, kind.name(), Internal::TypedArray(Box::new(v)))
}

/// AllocateTypedArray (ECMA-262 §23.2.5.1.1) with a new zeroed buffer of `len` elements.
fn alloc(it: &mut Interp, kind: Kind, len: usize) -> R<Obj> {
    let bytes = len as f64 * kind.size() as f64;
    if bytes > MAX_BYTES {
        return it.throw("RangeError", BAD_LENGTH);
    }
    let (b, data) = new_buffer(it, vec![0; bytes as usize]);
    Ok(new_view(it, kind, b, data, 0, len))
}

/// A typed array of `kind` with elements ToNumber(`items`), converted in order.
fn from_values(it: &mut Interp, kind: Kind, items: &[Value]) -> R<Obj> {
    let o = alloc(it, kind, items.len())?;
    for (i, x) in items.iter().enumerate() {
        let n = it.to_numeric(x)?;
        typed(&o).unwrap().write(i, n);
    }
    Ok(o)
}

/// Objects [`Interp::iterate`] iterates; any other object is taken as array-like.
fn iterable(o: &Obj) -> bool {
    let b = o.borrow();
    b.array.is_some()
        || b.internal.collection().is_some()
        || matches!(b.internal, Internal::TypedArray(_) | Internal::Arguments(_))
}

/// The elements of an iterable `src`, else of the array-like object `src` (ECMA-262
/// §23.2.5.1.4-5 and %TypedArray%.from).
fn elements(it: &mut Interp, src: &Value) -> R<Vec<Value>> {
    match src {
        Value::Obj(o) if !iterable(o) => {
            let n = it.get(src, "length")?;
            let n = integer(it, &n)?;
            let n = n.clamp(0.0, MAX_BYTES) as usize;
            (0..n).map(|i| it.get(src, &i.to_string())).collect()
        }
        Value::Undefined | Value::Null => it.throw(
            "TypeError",
            format!("{} is not an object", src.to_js_string()),
        ),
        Value::Num(_) | Value::Bool(_) => Ok(Vec::new()),
        _ => it.iterate(src),
    }
}

/// An `ArrayBuffer` without data: V4 makes one of a null QByteArray. Its `byteLength` is 0
/// and nothing can be made of it: a typed array, a `DataView` and `slice` throw.
pub(crate) fn detached_buffer(it: &Interp) -> Obj {
    with_internal(it, "ArrayBuffer", Internal::DetachedArrayBuffer)
}

fn detached(o: &Obj) -> bool {
    matches!(o.borrow().internal, Internal::DetachedArrayBuffer)
}

/// The bytes of an `ArrayBuffer` or `SharedArrayBuffer`.
fn buffer_data(o: &Obj) -> Option<Bytes> {
    match &o.borrow().internal {
        Internal::ArrayBuffer(d) | Internal::SharedArrayBuffer(d) => Some(d.clone()),
        _ => None,
    }
}

/// The typed array constructor of `kind` (ECMA-262 §23.2.5.1).
fn construct(it: &mut Interp, kind: Kind, a: &[Value]) -> R<Value> {
    let first = arg(a, 0);
    let Value::Obj(src) = &first else {
        let n = to_index(it, &first, "Index out of range.")?;
        return Ok(obj(alloc(it, kind, n)?));
    };
    if detached(src) {
        return it.throw("TypeError", "Type error");
    }
    if let Some(data) = buffer_data(src) {
        let size = kind.size();
        let offset = to_index(it, &arg(a, 1), BAD_OFFSET)?;
        let total = data.borrow().len();
        if offset % size != 0 || offset > total {
            return it.throw("RangeError", BAD_OFFSET);
        }
        let len = match arg(a, 2) {
            Value::Undefined if total % size != 0 => return it.throw("RangeError", BAD_LENGTH),
            Value::Undefined => (total - offset) / size,
            l => {
                let l = to_index(it, &l, BAD_LENGTH)?;
                if offset + l * size > total {
                    return it.throw("RangeError", BAD_LENGTH);
                }
                l
            }
        };
        return Ok(obj(new_view(it, kind, src.clone(), data, offset, len)));
    }
    let items = elements(it, &first)?;
    Ok(obj(from_values(it, kind, &items)?))
}

/// ValidateTypedArray (ECMA-262 §23.2.4.4) of `this`.
fn this_typed(it: &mut Interp, this: &Value) -> R<Obj> {
    match this {
        Value::Obj(o) if typed(o).is_some() => Ok(o.clone()),
        _ => it.throw("TypeError", "Type error"),
    }
}

fn view_of(o: &Obj) -> std::cell::Ref<'_, View> {
    typed(o).unwrap()
}

/// A callback argument, TypeError unless callable.
fn func(it: &mut Interp, v: &Value) -> R<Value> {
    if callable(v).is_none() {
        return it.type_error();
    }
    Ok(v.clone())
}

/// TypedArraySpeciesCreate (ECMA-262 §23.2.4.1) for the default constructor of `kind`.
/// TypedArraySpeciesCreate (ECMA-262 §23.2.4.1) of `o` with `args`: `None` for a new
/// array of `o`'s kind, else the typed array `o.constructor[@@species]` constructs (at least
/// as long as asked with a single length).
fn species_create(it: &mut Interp, o: &Obj, args: &[Value]) -> R<Option<Obj>> {
    let Some(c) = species_constructor(it, &obj(o.clone()))? else {
        return Ok(None);
    };
    let r = it.construct(&c, args)?;
    let r = this_typed(it, &r)?;
    if let [Value::Num(n)] = args
        && (view_of(&r).len as f64) < *n
    {
        return it.throw("TypeError", "TypedArray species constructor: too short");
    }
    Ok(Some(r))
}

/// `items` (`o`'s values) written to the [`species_create`] result `out`, or a new array of
/// `o`'s kind.
fn write_values(it: &mut Interp, out: Option<Obj>, o: &Obj, items: &[Value]) -> R<Value> {
    let Some(out) = out else {
        return Ok(obj(from_values(it, same_kind(o), items)?));
    };
    for (i, x) in items.iter().enumerate() {
        let n = it.to_numeric(x)?;
        view_of(&out).write(i, n);
    }
    Ok(obj(out))
}

fn same_kind(o: &Obj) -> Kind {
    view_of(o).kind
}

fn sort_cmp(x: f64, y: f64) -> Ordering {
    // ECMA-262 §23.2.4.7 TypedArray SortCompare without a comparator.
    match (x.is_nan(), y.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        _ if x == 0.0 && y == 0.0 => y.is_sign_negative().cmp(&x.is_sign_negative()),
        _ => x.partial_cmp(&y).unwrap(),
    }
}

/// Merge sort with a fallible comparator (Array.prototype.sort is stable, ECMA-262 §23.1.3.30).
fn sort_by(it: &mut Interp, v: Vec<f64>, cmp: &Value) -> R<Vec<f64>> {
    if v.len() < 2 {
        return Ok(v);
    }
    let mut right = v;
    let left: Vec<f64> = right.drain(..right.len() / 2).collect();
    let (l, r) = (sort_by(it, left, cmp)?, sort_by(it, right, cmp)?);
    let mut out = Vec::with_capacity(l.len() + r.len());
    let (mut i, mut j) = (0, 0);
    while i < l.len() && j < r.len() {
        let c = it.call(cmp, Value::Undefined, &[Value::Num(l[i]), Value::Num(r[j])])?;
        let c = it.to_numeric(&c)?;
        if c > 0.0 {
            out.push(r[j]);
            j += 1;
        } else {
            out.push(l[i]);
            i += 1;
        }
    }
    out.extend_from_slice(&l[i..]);
    out.extend_from_slice(&r[j..]);
    Ok(out)
}

fn join(it: &mut Interp, o: &Obj, sep: &Value) -> R<Value> {
    let sep = match sep {
        Value::Undefined => ",".to_string(),
        s => it.to_string(s)?,
    };
    let parts: Vec<String> = view_of(o)
        .numbers()
        .into_iter()
        .map(num_to_string)
        .collect();
    Ok(Value::Str(parts.join(&sep).into()))
}

/// The callback methods' loop: `f(element, index, array)` for each index in `order`.
fn each(
    it: &mut Interp,
    o: &Obj,
    a: &[Value],
    rev: bool,
    mut step: impl FnMut(f64, usize, &Value) -> Option<Value>,
) -> R<Option<Value>> {
    let f = func(it, &arg(a, 0))?;
    let this = arg(a, 1);
    let len = view_of(o).len;
    for k in 0..len {
        let i = if rev { len - 1 - k } else { k };
        let x = view_of(o).read(i).map_or(Value::Undefined, Value::Num);
        let r = it.call(
            &f,
            this.clone(),
            &[x.clone(), Value::Num(i as f64), obj(o.clone())],
        )?;
        if let Some(done) = step(x.to_number(), i, &r) {
            return Ok(Some(done));
        }
    }
    Ok(None)
}

fn method(p: &Obj, name: &'static str, length: u32, f: NativeFn) {
    set_hidden(p, name, native(name, length, f));
}

fn getter(p: &Obj, name: &'static str, f: NativeFn) {
    define_accessor(p, name, true, fn_value(Callable::Native(name, f)), false);
}

/// %TypedArray%.prototype (ECMA-262 §23.2.3).
fn install_typed_prototype(p: &Obj) {
    getter(p, "buffer", |it, this, _| {
        Ok(obj(view_of(&this_typed(it, this)?).buffer.clone()))
    });
    getter(p, "byteLength", |it, this, _| {
        Ok(Value::Num(view_of(&this_typed(it, this)?).byte_len() as f64))
    });
    getter(p, "byteOffset", |it, this, _| {
        Ok(Value::Num(view_of(&this_typed(it, this)?).offset as f64))
    });
    getter(p, "length", |it, this, _| {
        Ok(Value::Num(view_of(&this_typed(it, this)?).len as f64))
    });
    method(p, "at", 1, |it, this, a| {
        let o = this_typed(it, this)?;
        let len = view_of(&o).len as f64;
        let n = integer(it, &arg(a, 0))?;
        let k = if n < 0.0 { len + n } else { n };
        Ok(if (0.0..len).contains(&k) {
            view_of(&o)
                .read(k as usize)
                .map_or(Value::Undefined, Value::Num)
        } else {
            Value::Undefined
        })
    });
    method(p, "copyWithin", 2, |it, this, a| {
        let o = this_typed(it, this)?;
        let len = view_of(&o).len;
        let to = relative(it, &arg(a, 0), len, 0)?;
        let from = relative(it, &arg(a, 1), len, 0)?;
        let end = relative(it, &arg(a, 2), len, len)?;
        let count = end.saturating_sub(from).min(len - to);
        if count > 0 {
            let v = view_of(&o);
            let s = v.kind.size();
            let base = v.offset;
            v.data
                .borrow_mut()
                .copy_within(base + from * s..base + (from + count) * s, base + to * s);
        }
        Ok(this.clone())
    });
    method(p, "every", 1, |it, this, a| {
        let o = this_typed(it, this)?;
        let r = each(it, &o, a, false, |_, _, r| {
            (!r.truthy()).then_some(Value::Bool(false))
        })?;
        Ok(r.unwrap_or(Value::Bool(true)))
    });
    method(p, "some", 1, |it, this, a| {
        let o = this_typed(it, this)?;
        let r = each(it, &o, a, false, |_, _, r| {
            r.truthy().then_some(Value::Bool(true))
        })?;
        Ok(r.unwrap_or(Value::Bool(false)))
    });
    method(p, "forEach", 1, |it, this, a| {
        let o = this_typed(it, this)?;
        each(it, &o, a, false, |_, _, _| None)?;
        Ok(Value::Undefined)
    });
    method(p, "find", 1, |it, this, a| {
        let o = this_typed(it, this)?;
        let r = each(it, &o, a, false, |x, _, r| {
            r.truthy().then_some(Value::Num(x))
        })?;
        Ok(r.unwrap_or_default())
    });
    method(p, "findIndex", 1, |it, this, a| {
        let o = this_typed(it, this)?;
        let r = each(it, &o, a, false, |_, i, r| {
            r.truthy().then_some(Value::Num(i as f64))
        })?;
        Ok(r.unwrap_or(Value::Num(-1.0)))
    });
    method(p, "findLast", 1, |it, this, a| {
        let o = this_typed(it, this)?;
        let r = each(it, &o, a, true, |x, _, r| {
            r.truthy().then_some(Value::Num(x))
        })?;
        Ok(r.unwrap_or_default())
    });
    method(p, "findLastIndex", 1, |it, this, a| {
        let o = this_typed(it, this)?;
        let r = each(it, &o, a, true, |_, i, r| {
            r.truthy().then_some(Value::Num(i as f64))
        })?;
        Ok(r.unwrap_or(Value::Num(-1.0)))
    });
    method(p, "filter", 1, |it, this, a| {
        let o = this_typed(it, this)?;
        let mut kept = Vec::new();
        each(it, &o, a, false, |x, _, r| {
            if r.truthy() {
                kept.push(Value::Num(x));
            }
            None
        })?;
        let out = species_create(it, &o, &[Value::Num(kept.len() as f64)])?;
        write_values(it, out, &o, &kept)
    });
    method(p, "map", 1, |it, this, a| {
        let o = this_typed(it, this)?;
        let len = view_of(&o).len;
        let out = match species_create(it, &o, &[Value::Num(len as f64)])? {
            Some(out) => out,
            None => alloc(it, same_kind(&o), len)?,
        };
        let mut mapped = Vec::new();
        each(it, &o, a, false, |_, _, r| {
            mapped.push(r.clone());
            None
        })?;
        for (i, x) in mapped.iter().enumerate() {
            let n = it.to_numeric(x)?;
            view_of(&out).write(i, n);
        }
        Ok(obj(out))
    });
    method(p, "reduce", 1, |it, this, a| reduce(it, this, a, false));
    method(p, "reduceRight", 1, |it, this, a| reduce(it, this, a, true));
    method(p, "fill", 1, |it, this, a| {
        let o = this_typed(it, this)?;
        let n = it.to_numeric(&arg(a, 0))?;
        let len = view_of(&o).len;
        let start = relative(it, &arg(a, 1), len, 0)?;
        let end = relative(it, &arg(a, 2), len, len)?;
        let v = view_of(&o);
        (start..end).for_each(|i| v.write(i, n));
        Ok(this.clone())
    });
    method(p, "includes", 1, |it, this, a| {
        search(it, this, a, Search::Includes)
    });
    method(p, "indexOf", 1, |it, this, a| {
        search(it, this, a, Search::First)
    });
    method(p, "lastIndexOf", 1, |it, this, a| {
        search(it, this, a, Search::Last)
    });
    method(p, "join", 1, |it, this, a| {
        let o = this_typed(it, this)?;
        join(it, &o, &arg(a, 0))
    });
    method(p, "toString", 0, |it, this, _| {
        let o = this_typed(it, this)?;
        join(it, &o, &Value::Undefined)
    });
    method(p, "toLocaleString", 0, |it, this, _| {
        let o = this_typed(it, this)?;
        let mut parts = Vec::new();
        for n in view_of(&o).numbers() {
            let x = Value::Num(n);
            let f = it.get(&x, "toLocaleString")?;
            let s = it.call(&f, x, &[])?;
            parts.push(it.to_string(&s)?);
        }
        Ok(Value::Str(parts.join(",").into()))
    });
    method(p, "reverse", 0, |it, this, _| {
        let o = this_typed(it, this)?;
        let v = view_of(&o);
        let mut n = v.numbers();
        n.reverse();
        n.into_iter().enumerate().for_each(|(i, x)| v.write(i, x));
        Ok(this.clone())
    });
    method(p, "set", 1, |it, this, a| {
        let o = this_typed(it, this)?;
        let offset = integer(it, &arg(a, 1))?;
        if offset < 0.0 {
            return it.throw("RangeError", SET_RANGE);
        }
        let src = arg(a, 0);
        let items = match &src {
            Value::Obj(s) if typed(s).is_some() => view_of(s).values(),
            Value::Undefined | Value::Null => return it.no_object(&src),
            Value::Obj(_) => {
                let n = it.get(&src, "length")?;
                let n = integer(it, &n)?.clamp(0.0, MAX_BYTES) as usize;
                if offset + n as f64 > view_of(&o).len as f64 {
                    return it.throw("RangeError", SET_RANGE);
                }
                let mut v = Vec::with_capacity(n);
                for i in 0..n {
                    let x = it.get(&src, &i.to_string())?;
                    v.push(Value::Num(it.to_numeric(&x)?));
                }
                v
            }
            _ => Vec::new(),
        };
        if offset + items.len() as f64 > view_of(&o).len as f64 {
            return it.throw("RangeError", SET_RANGE);
        }
        for (i, x) in items.iter().enumerate() {
            let n = it.to_numeric(x)?;
            view_of(&o).write(offset as usize + i, n);
        }
        Ok(Value::Undefined)
    });
    method(p, "slice", 2, |it, this, a| {
        let o = this_typed(it, this)?;
        let len = view_of(&o).len;
        let start = relative(it, &arg(a, 0), len, 0)?;
        let end = relative(it, &arg(a, 1), len, len)?;
        let count = end.saturating_sub(start);
        let out = species_create(it, &o, &[Value::Num(count as f64)])?;
        let items: Vec<Value> = view_of(&o).values().drain(start.min(end)..end).collect();
        write_values(it, out, &o, &items)
    });
    method(p, "subarray", 2, |it, this, a| {
        let o = this_typed(it, this)?;
        let len = view_of(&o).len;
        let start = relative(it, &arg(a, 0), len, 0)?;
        let end = relative(it, &arg(a, 1), len, len)?;
        let (kind, buffer, data, offset) = {
            let v = view_of(&o);
            (
                v.kind,
                v.buffer.clone(),
                v.data.clone(),
                v.offset + start * v.kind.size(),
            )
        };
        let len = end.saturating_sub(start);
        let args = [
            obj(buffer.clone()),
            Value::Num(offset as f64),
            Value::Num(len as f64),
        ];
        if let Some(out) = species_create(it, &o, &args)? {
            return Ok(obj(out));
        }
        Ok(obj(new_view(
            it,
            kind,
            buffer,
            data,
            offset,
            end.saturating_sub(start),
        )))
    });
    method(p, "sort", 1, |it, this, a| {
        let o = this_typed(it, this)?;
        let cmp = arg(a, 0);
        let n = view_of(&o).numbers();
        let sorted = match cmp {
            Value::Undefined => {
                let mut n = n;
                n.sort_by(|x, y| sort_cmp(*x, *y));
                n
            }
            f => {
                let f = func(it, &f)?;
                sort_by(it, n, &f)?
            }
        };
        let v = view_of(&o);
        sorted
            .into_iter()
            .enumerate()
            .for_each(|(i, x)| v.write(i, x));
        Ok(this.clone())
    });
}

fn reduce(it: &mut Interp, this: &Value, a: &[Value], rev: bool) -> R<Value> {
    let o = this_typed(it, this)?;
    let f = func(it, &arg(a, 0))?;
    let len = view_of(&o).len;
    let mut order: Box<dyn Iterator<Item = usize>> = if rev {
        Box::new((0..len).rev())
    } else {
        Box::new(0..len)
    };
    let mut acc = match a.get(1) {
        Some(v) => v.clone(),
        None => match order.next() {
            Some(i) => Value::Num(view_of(&o).read(i).unwrap_or(f64::NAN)),
            None => return it.throw("TypeError", "Type error"),
        },
    };
    for i in order {
        let x = view_of(&o).read(i).map_or(Value::Undefined, Value::Num);
        acc = it.call(
            &f,
            Value::Undefined,
            &[acc, x, Value::Num(i as f64), obj(o.clone())],
        )?;
    }
    Ok(acc)
}

enum Search {
    Includes,
    First,
    Last,
}

fn search(it: &mut Interp, this: &Value, a: &[Value], how: Search) -> R<Value> {
    let o = this_typed(it, this)?;
    let len = view_of(&o).len;
    let miss = match how {
        Search::Includes => Value::Bool(false),
        _ => Value::Num(-1.0),
    };
    if len == 0 {
        return Ok(miss);
    }
    let range: Vec<usize> = match how {
        Search::Last => {
            let from = match a.get(1) {
                None => len as f64 - 1.0,
                Some(v) => integer(it, v)?,
            };
            let from = if from < 0.0 {
                len as f64 + from
            } else {
                from.min(len as f64 - 1.0)
            };
            if from < 0.0 {
                return Ok(miss);
            }
            (0..=from as usize).rev().collect()
        }
        _ => (relative(it, &arg(a, 1), len, 0)?..len).collect(),
    };
    let Value::Num(x) = arg(a, 0) else {
        return Ok(miss);
    };
    let v = view_of(&o);
    for i in range {
        let e = v.read(i).unwrap_or(f64::NAN);
        let hit = match how {
            Search::Includes => e == x || (e.is_nan() && x.is_nan()),
            _ => e == x,
        };
        if hit {
            return Ok(match how {
                Search::Includes => Value::Bool(true),
                _ => Value::Num(i as f64),
            });
        }
    }
    Ok(miss)
}

/// The typed array kind constructor `this` (`Uint8Array.from(..)`) makes.
fn this_kind(it: &mut Interp, this: &Value) -> R<Kind> {
    match callable(this) {
        Some(Callable::Native(name, _)) if let Some(k) = Kind::from_name(name) => Ok(k),
        _ => it.throw("TypeError", "Type error"),
    }
}

fn this_view(it: &mut Interp, this: &Value) -> R<Obj> {
    match this {
        Value::Obj(o) if matches!(o.borrow().internal, Internal::DataView(_)) => Ok(o.clone()),
        _ => it.throw("TypeError", "Type error"),
    }
}

fn data_view(o: &Obj) -> std::cell::Ref<'_, View> {
    std::cell::Ref::map(o.borrow(), |b| match &b.internal {
        Internal::DataView(v) => &**v,
        _ => unreachable!("checked by this_view"),
    })
}

/// GetViewValue (ECMA-262 §25.3.1.5).
fn view_get(it: &mut Interp, this: &Value, a: &[Value], kind: Kind) -> R<Value> {
    let o = this_view(it, this)?;
    let at = to_index(it, &arg(a, 0), OUT_OF_RANGE)?;
    let little = arg(a, 1).truthy();
    let v = data_view(&o);
    if at + kind.size() > v.len {
        drop(v);
        return it.throw("RangeError", OUT_OF_RANGE);
    }
    let s = v.offset + at;
    Ok(Value::Num(
        kind.decode(&v.data.borrow()[s..s + kind.size()], little),
    ))
}

/// SetViewValue (ECMA-262 §25.3.1.6).
fn view_set(it: &mut Interp, this: &Value, a: &[Value], kind: Kind) -> R<Value> {
    let o = this_view(it, this)?;
    let at = to_index(it, &arg(a, 0), OUT_OF_RANGE)?;
    let n = it.to_numeric(&arg(a, 1))?;
    let little = arg(a, 2).truthy();
    let v = data_view(&o);
    if at + kind.size() > v.len {
        drop(v);
        return it.throw("RangeError", OUT_OF_RANGE);
    }
    let s = v.offset + at;
    kind.encode(&mut v.data.borrow_mut()[s..s + kind.size()], n, little);
    Ok(Value::Undefined)
}

fn install_data_view(def: &mut dyn FnMut(&'static str, Value)) {
    let ctor = native("DataView", 1, |it, _, a| {
        if !std::mem::take(&mut it.constructing) {
            return it.type_error();
        }
        let buf = arg(a, 0);
        let Some((b, data)) = (match &buf {
            Value::Obj(b) => buffer_data(b).map(|d| (b.clone(), d)),
            _ => None,
        }) else {
            // V4's words, also for a buffer without data.
            return it.throw("TypeError", "Type error");
        };
        let offset = to_index(it, &arg(a, 1), VIEW_RANGE)?;
        let total = data.borrow().len();
        if offset > total {
            return it.throw("RangeError", VIEW_RANGE);
        }
        let len = match arg(a, 2) {
            Value::Undefined => total - offset,
            l => {
                let l = to_index(it, &l, VIEW_RANGE)?;
                if offset + l > total {
                    return it.throw("RangeError", VIEW_RANGE);
                }
                l
            }
        };
        let v = View {
            kind: Kind::U8,
            buffer: b,
            data,
            offset,
            len,
        };
        Ok(obj(with_internal(
            it,
            "DataView",
            Internal::DataView(Box::new(v)),
        )))
    });
    let p = new_obj();
    getter(&p, "buffer", |it, this, _| {
        Ok(obj(data_view(&this_view(it, this)?).buffer.clone()))
    });
    getter(&p, "byteLength", |it, this, _| {
        Ok(Value::Num(data_view(&this_view(it, this)?).len as f64))
    });
    getter(&p, "byteOffset", |it, this, _| {
        Ok(Value::Num(data_view(&this_view(it, this)?).offset as f64))
    });
    macro_rules! accessors {
        ($($k:ident $get:literal $set:literal),*) => {$(
            method(&p, $get, 1, |it, this, a| view_get(it, this, a, Kind::$k));
            method(&p, $set, 2, |it, this, a| view_set(it, this, a, Kind::$k));
        )*};
    }
    accessors!(
        I8 "getInt8" "setInt8", U8 "getUint8" "setUint8",
        I16 "getInt16" "setInt16", U16 "getUint16" "setUint16",
        I32 "getInt32" "setInt32", U32 "getUint32" "setUint32",
        F32 "getFloat32" "setFloat32", F64 "getFloat64" "setFloat64",
        // V4's spellings, kept for scripts written against Qt.
        U8 "getUInt8" "setUInt8", U16 "getUInt16" "setUInt16", U32 "getUInt32" "setUInt32"
    );
    if let Value::Obj(f) = &ctor {
        link_builtin_prototype(f, &p);
    }
    def("DataView", ctor);
}

/// The bytes of `this` when it is an `ArrayBuffer` (`shared`: a `SharedArrayBuffer`).
fn this_buffer(it: &mut Interp, this: &Value, shared: bool) -> R<Bytes> {
    if let Value::Obj(o) = this {
        match &o.borrow().internal {
            Internal::ArrayBuffer(d) if !shared => return Ok(d.clone()),
            Internal::SharedArrayBuffer(d) if shared => return Ok(d.clone()),
            _ => {}
        }
    }
    it.throw("TypeError", "Type error")
}

/// A new `ArrayBuffer` (`shared`: `SharedArrayBuffer`) holding `bytes`.
fn make_buffer(it: &Interp, bytes: Vec<u8>, shared: bool) -> Obj {
    if !shared {
        return new_buffer(it, bytes).0;
    }
    let data = Rc::new(RefCell::new(bytes));
    with_internal(it, "SharedArrayBuffer", Internal::SharedArrayBuffer(data))
}

/// `ArrayBuffer.prototype.slice` and `SharedArrayBuffer.prototype.slice` (ECMA-262
/// §25.1.6.6, §25.2.5.6).
fn buffer_slice(it: &mut Interp, this: &Value, a: &[Value], shared: bool) -> R<Value> {
    let d = this_buffer(it, this, shared)?;
    let len = d.borrow().len();
    let start = relative(it, &arg(a, 0), len, 0)?;
    let end = relative(it, &arg(a, 1), len, len)?;
    let n = end.saturating_sub(start);
    let Some(c) = species_constructor(it, this)? else {
        let bytes = d.borrow()[start.min(end)..end].to_vec();
        return Ok(obj(make_buffer(it, bytes, shared)));
    };
    let r = it.construct(&c, &[Value::Num(n as f64)])?;
    let out = match &r {
        Value::Obj(_) => this_buffer(it, &r, shared)?,
        _ => return it.throw("TypeError", "ArrayBuffer species constructor: not a buffer"),
    };
    if Rc::ptr_eq(&out, &d) {
        return it.throw("TypeError", "ArrayBuffer species constructor: same buffer");
    }
    if out.borrow().len() < n {
        return it.throw("TypeError", "ArrayBuffer species constructor: too short");
    }
    // The source may have shrunk (been detached) by the constructor.
    let src = d.borrow();
    let end = end.min(src.len());
    let copied = &src[start.min(end)..end];
    out.borrow_mut()[..copied.len()].copy_from_slice(copied);
    drop(src);
    Ok(r)
}

/// `ArrayBuffer` (§25.1) or, `shared`, `SharedArrayBuffer` (§25.2).
fn install_buffer(def: &mut dyn FnMut(&'static str, Value), shared: bool) {
    let (name, ctor): (_, NativeFn) = if shared {
        ("SharedArrayBuffer", |it, _, a| {
            let n = to_index(
                it,
                &arg(a, 0),
                "SharedArrayBuffer constructor: invalid length",
            )?;
            Ok(obj(make_buffer(it, vec![0; n], true)))
        })
    } else {
        ("ArrayBuffer", |it, _, a| {
            if !std::mem::take(&mut it.constructing) {
                return it.type_error();
            }
            let n = to_index(it, &arg(a, 0), "ArrayBuffer constructor: invalid length")?;
            Ok(obj(make_buffer(it, vec![0; n], false)))
        })
    };
    let ctor = native(name, 1, ctor);
    let Value::Obj(f) = &ctor else { unreachable!() };
    let p = new_obj();
    if shared {
        getter(&p, "byteLength", |it, this, _| {
            Ok(Value::Num(
                this_buffer(it, this, true)?.borrow().len() as f64
            ))
        });
        method(&p, "slice", 2, |it, this, a| {
            buffer_slice(it, this, a, true)
        });
    } else {
        method(f, "isView", 1, |_, _, a| {
            Ok(Value::Bool(matches!(&arg(a, 0), Value::Obj(o) if matches!(
                o.borrow().internal,
                Internal::TypedArray(_) | Internal::DataView(_)
            ))))
        });
        getter(&p, "byteLength", |it, this, _| {
            if matches!(this, Value::Obj(o) if detached(o)) {
                return Ok(Value::Num(0.0));
            }
            Ok(Value::Num(
                this_buffer(it, this, false)?.borrow().len() as f64
            ))
        });
        method(&p, "slice", 2, |it, this, a| {
            buffer_slice(it, this, a, false)
        });
        // V4's own (ArrayBufferPrototype::method_toString): the bytes read as UTF-8.
        method(&p, "toString", 0, |_, this, _| {
            Ok(match this {
                Value::Obj(o) => match &o.borrow().internal {
                    Internal::ArrayBuffer(d) => Value::str(&String::from_utf8_lossy(&d.borrow())),
                    Internal::DetachedArrayBuffer => Value::str(""),
                    _ => Value::Undefined,
                },
                _ => Value::Undefined,
            })
        });
    }
    link_builtin_prototype(f, &p);
    def(name, ctor);
}

/// ValidateIntegerTypedArray and ValidateAtomicAccess (ECMA-262 §25.4.3.1-2): the view and
/// element index an `Atomics` operation works on; `waitable`: an `Int32Array` only.
fn atomic_access(it: &mut Interp, a: &[Value], waitable: bool) -> R<(Obj, usize)> {
    let ok = match &arg(a, 0) {
        Value::Obj(o) => typed(o).is_some_and(|v| match v.kind {
            Kind::I32 => true,
            Kind::I8 | Kind::U8 | Kind::I16 | Kind::U16 | Kind::U32 => !waitable,
            _ => false,
        }),
        _ => false,
    };
    let Value::Obj(o) = arg(a, 0) else {
        return it.throw("TypeError", "Type error");
    };
    if !ok {
        return it.throw("TypeError", "Type error");
    }
    let i = to_index(it, &arg(a, 1), OUT_OF_RANGE)?;
    if i >= view_of(&o).len {
        return it.throw("RangeError", OUT_OF_RANGE);
    }
    Ok((o, i))
}

/// AtomicReadModifyWrite (ECMA-262 §25.4.3.17): stores `op(old, v)`, returns the old element.
fn atomic_rmw(it: &mut Interp, a: &[Value], op: fn(f64, f64) -> f64) -> R<Value> {
    let (o, i) = atomic_access(it, a, false)?;
    let v = integer(it, &arg(a, 2))?;
    let t = view_of(&o);
    let old = t.read(i).unwrap_or(0.0);
    t.write(i, op(old, v));
    Ok(Value::Num(old))
}

/// The bitwise operations on the elements' integer values (both operands taken modulo 2^32,
/// which keeps every element type's low bits).
fn bits(x: f64, y: f64, f: fn(u32, u32) -> u32) -> f64 {
    f(modular(x, 32) as u32, modular(y, 32) as u32) as f64
}

/// The `Atomics` namespace (ECMA-262 §25.4). This runtime has a single agent, which cannot
/// suspend (AgentCanSuspend is false), so `wait` throws and `notify` wakes no one.
fn install_atomics(def: &mut dyn FnMut(&'static str, Value)) {
    let at = new_obj();
    method(&at, "add", 3, |it, _, a| atomic_rmw(it, a, |x, y| x + y));
    method(&at, "sub", 3, |it, _, a| atomic_rmw(it, a, |x, y| x - y));
    method(&at, "and", 3, |it, _, a| {
        atomic_rmw(it, a, |x, y| bits(x, y, |x, y| x & y))
    });
    method(&at, "or", 3, |it, _, a| {
        atomic_rmw(it, a, |x, y| bits(x, y, |x, y| x | y))
    });
    method(&at, "xor", 3, |it, _, a| {
        atomic_rmw(it, a, |x, y| bits(x, y, |x, y| x ^ y))
    });
    method(&at, "exchange", 3, |it, _, a| atomic_rmw(it, a, |_, y| y));
    method(&at, "compareExchange", 4, |it, _, a| {
        let (o, i) = atomic_access(it, a, false)?;
        let expected = integer(it, &arg(a, 2))?;
        let replacement = integer(it, &arg(a, 3))?;
        let t = view_of(&o);
        let s = t.kind.size();
        let mut want = vec![0; s];
        t.kind
            .encode(&mut want, expected, cfg!(target_endian = "little"));
        let at = t.offset + i * s;
        let old = t.read(i).unwrap_or(0.0);
        if t.data.borrow()[at..at + s] == want[..] {
            t.write(i, replacement);
        }
        Ok(Value::Num(old))
    });
    method(&at, "load", 2, |it, _, a| {
        let (o, i) = atomic_access(it, a, false)?;
        Ok(Value::Num(view_of(&o).read(i).unwrap_or(0.0)))
    });
    method(&at, "store", 3, |it, _, a| {
        let (o, i) = atomic_access(it, a, false)?;
        let v = integer(it, &arg(a, 2))?;
        view_of(&o).write(i, v);
        Ok(Value::Num(v))
    });
    // V4 6.12 answers true for 1 and 4 bytes (4 must be lock-free, ECMA-262 §25.4.6).
    method(&at, "isLockFree", 1, |it, _, a| {
        let n = integer(it, &arg(a, 0))?;
        Ok(Value::Bool(n == 1.0 || n == 4.0))
    });
    method(&at, "wait", 4, |it, _, a| {
        let (o, _) = atomic_access(it, a, true)?;
        if !matches!(
            view_of(&o).buffer.borrow().internal,
            Internal::SharedArrayBuffer(_)
        ) {
            return it.throw("TypeError", "Type error");
        }
        integer(it, &arg(a, 2))?;
        it.to_numeric(&arg(a, 3))?;
        it.throw("TypeError", "Atomics.wait cannot be called in this context")
    });
    let notify: NativeFn = |it, _, a| {
        atomic_access(it, a, true)?;
        if !matches!(arg(a, 2), Value::Undefined) {
            integer(it, &arg(a, 2))?;
        }
        Ok(Value::Num(0.0))
    };
    set_hidden(&at, "notify", native("notify", 3, notify));
    // ES2017's name for `notify`, the one V4 6.12 has.
    set_hidden(&at, "wake", native("wake", 3, notify));
    def("Atomics", obj(at));
}

/// Installs `ArrayBuffer`, `DataView`, %TypedArray% and its nine constructors through `def`.
/// `array_to_string`: %Array.prototype.toString%, which %TypedArray%.prototype shares
/// (ECMA-262 §23.2.3.32).
/// `function_proto`: %Function.prototype%.
pub fn install(
    def: &mut dyn FnMut(&'static str, Value),
    array_to_string: Option<Value>,
    function_proto: Obj,
) {
    install_buffer(def, false);
    install_buffer(def, true);
    install_data_view(def);
    install_atomics(def);
    // QV4's %TypedArray% is an object of class Function that cannot be called or constructed
    // (ECMA-262 §23.2.1 makes it a function that throws): `typeof` answers "object" and a
    // call is QV4's TypeError for a value that is not a function.
    let base = new_obj();
    base.borrow_mut().proto = Some(function_proto);
    for (k, v) in [
        ("length", Value::Num(0.0)),
        ("name", Value::str("TypedArray")),
    ] {
        set_hidden(&base, k, v);
        base.borrow_mut().set_attr(k, crate::value::READONLY);
    }
    let base_proto = new_obj();
    install_typed_prototype(&base_proto);
    if let Some(f) = array_to_string {
        set_hidden(&base_proto, "toString", f);
    }
    link_builtin_prototype(&base, &base_proto);
    // get %TypedArray%.prototype[@@toStringTag] (ECMA-262 §23.2.3.38): the name of a typed
    // array's constructor, `undefined` for anything else.
    crate::eval::define_sym_accessor(
        &base_proto,
        &crate::value::WellKnown::ToStringTag.sym(),
        true,
        fn_value(Callable::Native(
            "get [Symbol.toStringTag]",
            |_, this, _| {
                Ok(match this {
                    Value::Obj(o) => match &o.borrow().internal {
                        Internal::TypedArray(t) => Value::str(t.kind.name()),
                        _ => Value::Undefined,
                    },
                    _ => Value::Undefined,
                })
            },
        )),
        false,
    );
    method(&base, "from", 1, |it, this, a| {
        let kind = this_kind(it, this)?;
        let map = match arg(a, 1) {
            Value::Undefined => None,
            f => Some(func(it, &f)?),
        };
        let mut items = elements(it, &arg(a, 0))?;
        if let Some(f) = map {
            let this_arg = arg(a, 2);
            for (i, x) in items.iter_mut().enumerate() {
                *x = it.call(&f, this_arg.clone(), &[x.clone(), Value::Num(i as f64)])?;
            }
        }
        Ok(obj(from_values(it, kind, &items)?))
    });
    method(&base, "of", 0, |it, this, a| {
        let kind = this_kind(it, this)?;
        Ok(obj(from_values(it, kind, a)?))
    });
    macro_rules! ctors {
        ($($k:ident $name:literal),*) => {[$(
            (Kind::$k, native($name, 3, |it, _, a| {
                if !std::mem::take(&mut it.constructing) {
                    return it.throw(
                        "TypeError",
                        "calling a TypedArray constructor without new is invalid",
                    );
                }
                construct(it, Kind::$k, a)
            })),
        )*]};
    }
    let all = ctors!(
        I8 "Int8Array", U8 "Uint8Array", U8C "Uint8ClampedArray", I16 "Int16Array",
        U16 "Uint16Array", I32 "Int32Array", U32 "Uint32Array", F32 "Float32Array",
        F64 "Float64Array"
    );
    for (kind, ctor) in all {
        let Value::Obj(f) = &ctor else { unreachable!() };
        let p = new_obj();
        p.borrow_mut().proto = Some(base_proto.clone());
        f.borrow_mut().proto = Some(base.clone());
        let bpe = Value::Num(kind.size() as f64);
        // Read-only and non-configurable (ECMA-262 §23.2.6.1, §23.2.7.1).
        for o in [f, &p] {
            set_hidden(o, "BYTES_PER_ELEMENT", bpe.clone());
            o.borrow_mut().set_attr(
                "BYTES_PER_ELEMENT",
                crate::value::READONLY | crate::value::PERMANENT,
            );
        }
        link_builtin_prototype(f, &p);
        def(kind.name(), ctor);
    }
}

/// `Array.prototype[name]` of `it`'s installed globals.
pub fn array_proto_member(it: &Interp, name: &str) -> Option<Value> {
    intrinsic_proto(it, "Array").and_then(|p| p.borrow().props.get(name).cloned())
}

/// Expected values are Qt 6.12's (PySide6 6.12 QML probe), except where noted as ECMA-262.
#[cfg(all(test, feature = "interp"))]
mod tests {
    use crate::eval::{Ex, Interp};

    fn ev(src: &str) -> String {
        let body = crate::ir::lower_program(src).unwrap();
        let mut it = Interp::default();
        let g = it.global.clone();
        match it.run(&body, &g) {
            Ok(v) => v.to_js_string(),
            Err(Ex::Throw(e)) => {
                let name = it.get(&e, "name").unwrap().to_js_string();
                format!("{name}: {}", it.get(&e, "message").unwrap().to_js_string())
            }
            Err(Ex::Short) => "SHORT".into(),
        }
    }

    fn check(cases: &[(&str, &str)]) {
        for (src, want) in cases {
            assert_eq!(ev(&format!("return {src}")), *want, "{src}");
        }
    }

    #[test]
    fn element_conversion_and_clamping() {
        check(&[
            ("new Uint8Array([1,2,3]).join()", "1,2,3"),
            (
                "String(new Float32Array([1.1, 0.5]))",
                "1.100000023841858,0.5",
            ),
            (
                "String(new Uint8ClampedArray([300, -5, 1.5, 2.5, 0.5, NaN, '7']))",
                "255,0,2,2,0,0,7",
            ),
            (
                "String(new Int8Array([128, 255, -129, 3.9, -3.9, Infinity]))",
                "-128,-1,127,3,-3,0",
            ),
            (
                "String(new Uint32Array([-1, 4294967296.5]))",
                "4294967295,0",
            ),
            ("String(new Int16Array([32768, 65535]))", "-32768,-1"),
            ("String(Int16Array.of(1, -1, 70000))", "1,-1,4464"),
            ("String(new Uint8Array(new Int16Array([257, -1])))", "1,255"),
            ("new Float32Array([16777217])[0]", "16777216"),
            ("new Float64Array([1.5, -0]).join()", "1.5,0"),
            (
                "(function () { var x = new Uint8Array([1]); x[0] = {valueOf: function () { return 42 }}; return x[0] })()",
                "42",
            ),
        ]);
    }

    #[test]
    fn indexed_access() {
        // `a['1.5']` is a canonical numeric key, never an own property (ECMA-262 §10.4.5;
        // V4 6.12 stores it as an ordinary property).
        check(&[
            (
                "(function () { var a = new Uint8Array(4); a[0] = 257; a[1] = '12'; a[5] = 9; a[-1] = 3; a['1.5'] = 4; return [a.join(), a[5], a[-1], a['1.5'], a.length].join('|') })()",
                "1,12,0,0||||4",
            ),
            (
                "(function () { var a = new Uint8Array(2); a.length = 5; return a.length })()",
                "2",
            ),
            (
                "[1 in new Uint8Array(2), 2 in new Uint8Array(2)].join()",
                "true,false",
            ),
            ("Object.keys(new Uint8Array([7, 8])).join()", "0,1"),
            (
                "(function () { var s = []; for (var k in new Uint8Array([7, 8])) s.push(k); return s.join() })()",
                "0,1",
            ),
            ("new Uint8Array([7, 8]).hasOwnProperty(1)", "true"),
        ]);
    }

    #[test]
    fn constructors() {
        check(&[
            ("new Uint8Array(1.5).length", "1"),
            ("new Uint8Array('3').length", "3"),
            ("new Uint8Array(null).length", "0"),
            ("new Uint8Array(true).length", "1"),
            ("new Uint8Array(-1)", "RangeError: Index out of range."),
            ("String(new Uint8Array({length: 2}))", "0,0"),
            ("String(new Uint8Array([1, , 3]))", "1,0,3"),
            (
                "String(Uint8Array.from([1, 2, 3], function (x) { return x * 2 }))",
                "2,4,6",
            ),
            ("String(Uint8Array.from('123'))", "1,2,3"),
            ("String(Uint8Array.from({length: 2, 0: 5, 1: 6}))", "5,6"),
            ("Uint8Array.from.call(Array, [1])", "TypeError: Type error"),
            (
                "(function () { var b = new ArrayBuffer(8); var u = new Uint8Array(b, 2, 3); return [u.byteOffset, u.byteLength, u.length, u.buffer === b].join() })()",
                "2,3,3,true",
            ),
            (
                "new Int32Array(new ArrayBuffer(8), 2)",
                "RangeError: new TypedArray: invalid byteOffset",
            ),
            (
                "new Int32Array(new ArrayBuffer(8), 12)",
                "RangeError: new TypedArray: invalid byteOffset",
            ),
            (
                "new Int32Array(new ArrayBuffer(7))",
                "RangeError: new TypedArray: invalid length",
            ),
            (
                "new Int32Array(new ArrayBuffer(8), 4, 2)",
                "RangeError: new TypedArray: invalid length",
            ),
            (
                "Uint8Array.BYTES_PER_ELEMENT + ',' + Float64Array.prototype.BYTES_PER_ELEMENT",
                "1,8",
            ),
            ("new Uint16Array(2).byteLength", "4"),
            (
                "(function () { var r = []; for (var k in new Uint8Array([7, 8])) r.push(k); for (var j in new DataView(new ArrayBuffer(2))) r.push(j); return r.join(); })()",
                "0,1",
            ),
            (
                "[Uint8Array.from.length, Uint8Array.of.length, Uint8Array.length, Uint8Array.prototype.set.length, Uint8Array.prototype.indexOf.length, Uint8Array.name].join()",
                "1,0,3,1,1,Uint8Array",
            ),
            (
                "Int8Array.prototype.join === Float64Array.prototype.join",
                "true",
            ),
            ("Int8Array.prototype.hasOwnProperty('join')", "false"),
            (
                "Int8Array.prototype.isPrototypeOf(new Int8Array(1))",
                "true",
            ),
            ("new Uint8Array(1).constructor === Uint8Array", "true"),
            ("new Int8Array(1) instanceof Uint8Array", "false"),
            ("Uint8Array.prototype.length", "TypeError: Type error"),
        ]);
    }

    #[test]
    fn prototype_methods() {
        check(&[
            ("String(new Uint8Array([1, 2, 3, 4]).subarray(1, 3))", "2,3"),
            (
                "(function () { var a = new Uint8Array([1, 2, 3, 4]); a.subarray(1)[0] = 9; return a.join() })()",
                "1,9,3,4",
            ),
            ("new Int16Array([1, 2, 3]).subarray(-2).byteOffset", "2"),
            ("String(new Uint8Array([1, 2, 3, 4]).slice(-2))", "3,4"),
            (
                "String(new Uint8Array([1, 2, 3]).map(function (x) { return x * 100 }))",
                "100,200,44",
            ),
            (
                "String(new Uint8Array([1, 2, 3]).filter(function (x) { return x > 1 }))",
                "2,3",
            ),
            (
                "new Uint8Array([1, 2, 3]).reduce(function (a, b) { return a + b })",
                "6",
            ),
            (
                "new Uint8Array([1, 2, 3]).reduceRight(function (a, b) { return a + '-' + b }, 'x')",
                "x-3-2-1",
            ),
            (
                "new Uint8Array(0).reduce(function (a) { return a })",
                "TypeError: Type error",
            ),
            ("new Uint8Array([1, 2, 3]).indexOf(2)", "1"),
            ("new Uint8Array([1, 2, 3]).indexOf(1, -2)", "-1"),
            ("new Uint8Array([2]).indexOf('2')", "-1"),
            ("new Float32Array([NaN]).includes(NaN)", "true"),
            ("new Float32Array([NaN]).indexOf(NaN)", "-1"),
            ("new Uint8Array([1, 2, 3]).includes(2, 2)", "false"),
            ("new Uint8Array([1, 2, 3]).lastIndexOf(3, -2)", "-1"),
            ("new Uint8Array([1, 2, 3]).join('-')", "1-2-3"),
            ("String(new Uint8Array([1, 2, 3]).reverse())", "3,2,1"),
            ("String(new Uint8Array(5).fill(7, 1, -1))", "0,7,7,7,0"),
            ("String(new Int8Array(2).fill('3'))", "3,3"),
            (
                "String(new Uint8Array([1, 2, 3, 4, 5]).copyWithin(0, 3))",
                "4,5,3,4,5",
            ),
            (
                "(function () { var a = new Uint8Array(4); a.set([1, 2], 1); a.set(new Int8Array([-1]), 3); return a.join() })()",
                "0,1,2,255",
            ),
            (
                "(function () { var u = new Uint8Array([1, 2, 3]); u.set(u.subarray(0, 2), 1); return u.join() })()",
                "1,1,2",
            ),
            (
                "new Uint8Array(2).set([1, 2, 3])",
                "RangeError: TypedArray.set: out of range",
            ),
            (
                "new Uint8Array([10, 20]).find(function (x) { return x > 15 })",
                "20",
            ),
            (
                "new Uint8Array([10, 20]).findIndex(function (x) { return x > 25 })",
                "-1",
            ),
            (
                "new Uint8Array([1, 2]).some(function (x) { return x > 1 })",
                "true",
            ),
            (
                "new Uint8Array(3).every(function () { return true })",
                "true",
            ),
            ("new Uint8Array([1]).map(5)", "TypeError: Type error"),
            (
                "(function () { var s = ''; new Uint8Array([1, 2]).forEach(function (v, i, a) { s += this.p + v + i + (a instanceof Uint8Array) }, {p: 'x'}); return s })()",
                "x10truex21true",
            ),
            ("new Uint8Array([1, 2]).toLocaleString()", "1,2"),
            ("'' + new Uint8Array([1, 2])", "1,2"),
            ("new Uint8Array([5]) + 1", "51"),
            (
                "Uint8Array.prototype.toString === Array.prototype.toString",
                "true",
            ),
            // ECMA-262 2023 methods V4 6.12 lacks.
            ("new Uint8Array([1, 2, 3]).at(-1)", "3"),
            (
                "String(new Float64Array([3, NaN, -0, 0, -1]).sort())",
                "-1,0,0,3,NaN",
            ),
            ("1 / new Float64Array([0, -0]).sort()[0]", "-Infinity"),
            (
                "String(new Int8Array([1, 2, 3]).sort(function (a, b) { return b - a }))",
                "3,2,1",
            ),
            (
                "new Uint8Array([1, 2, 3]).findLast(function (x) { return x < 3 })",
                "2",
            ),
        ]);
    }

    #[test]
    fn array_json_and_tags() {
        check(&[
            (
                "JSON.stringify(new Uint8Array([1, 2]))",
                "{\"0\":1,\"1\":2}",
            ),
            (
                "JSON.stringify({a: new Int8Array([1])})",
                "{\"a\":{\"0\":1}}",
            ),
            ("JSON.stringify(new ArrayBuffer(2))", "{}"),
            ("JSON.stringify(new DataView(new ArrayBuffer(2)))", "{}"),
            (
                "(function () { var a = new Uint8Array(2); a.foo = 3; return JSON.stringify(a) + Object.keys(a).join() })()",
                "{\"0\":0,\"1\":0,\"foo\":3}0,1,foo",
            ),
            ("Array.from(new Uint8Array([4, 5])).join()", "4,5"),
            ("[].concat(new Uint8Array([4, 5])).length", "1"),
            ("Array.isArray(new Uint8Array(1))", "false"),
            ("[...new Uint8Array([7, 8])].join()", "7,8"),
            (
                "Object.prototype.toString.call(new ArrayBuffer(1))",
                "[object ArrayBuffer]",
            ),
            (
                "Object.prototype.toString.call(new SharedArrayBuffer(1))",
                "[object SharedArrayBuffer]",
            ),
            (
                "Object.prototype.toString.call(new DataView(new ArrayBuffer(1)))",
                "[object DataView]",
            ),
            (
                "Object.prototype.toString.call(new Float64Array(1))",
                "[object Float64Array]",
            ),
            (
                "Object.prototype.toString.call(new Uint8ClampedArray(1))",
                "[object Uint8ClampedArray]",
            ),
        ]);
    }

    #[test]
    fn array_buffer_and_data_view() {
        check(&[
            ("new ArrayBuffer(8).byteLength", "8"),
            ("new ArrayBuffer(1.7).byteLength", "1"),
            ("new ArrayBuffer('4').byteLength", "4"),
            ("new ArrayBuffer().byteLength", "0"),
            (
                "new ArrayBuffer(-1)",
                "RangeError: ArrayBuffer constructor: invalid length",
            ),
            ("new ArrayBuffer(8).slice(2, -1).byteLength", "5"),
            ("new ArrayBuffer(4).slice(3, 1).byteLength", "0"),
            (
                "ArrayBuffer.isView(new DataView(new ArrayBuffer(1)))",
                "true",
            ),
            (
                "ArrayBuffer.isView(new Int8Array(new SharedArrayBuffer(4)))",
                "true",
            ),
            ("ArrayBuffer.isView([])", "false"),
            ("new SharedArrayBuffer(4).slice(1).byteLength", "3"),
            (
                "ArrayBuffer.prototype.slice.call(new SharedArrayBuffer(2))",
                "TypeError: Type error",
            ),
            (
                "SharedArrayBuffer.prototype.slice.call(new ArrayBuffer(2))",
                "TypeError: Type error",
            ),
            (
                "(function () { var d = new DataView(new ArrayBuffer(8)); d.setInt16(0, -2); d.setFloat32(4, 1.5, true); return [d.getInt16(0), d.getUint16(0), d.getUint8(0), d.getFloat32(4, true), d.getFloat32(4)].join() })()",
                "-2,65534,255,1.5,6.896490392174587e-41",
            ),
            (
                "(function () { var d = new DataView(new ArrayBuffer(8)); d.setUint32(0, 0x01020304); return [d.getUInt8(0), d.getUint8(3), d.getUInt32(0, true)].join() })()",
                "1,4,67305985",
            ),
            (
                "(function () { var d = new DataView(new ArrayBuffer(4), 1); return [d.byteOffset, d.byteLength].join() })()",
                "1,3",
            ),
            ("new DataView(new ArrayBuffer(2)).getInt8()", "0"),
            ("new DataView(new SharedArrayBuffer(4)).byteLength", "4"),
            (
                "new DataView(new ArrayBuffer(4)).getInt32(1)",
                "RangeError: index out of range",
            ),
            (
                "new DataView(new ArrayBuffer(4)).getInt8(-1)",
                "RangeError: index out of range",
            ),
            (
                "new DataView(new ArrayBuffer(2)).setInt8(5, 1)",
                "RangeError: index out of range",
            ),
            (
                "new DataView(new ArrayBuffer(4), 5)",
                "RangeError: DataView: constructor arguments out of range",
            ),
            (
                "new DataView(new ArrayBuffer(4), 1, 4)",
                "RangeError: DataView: constructor arguments out of range",
            ),
        ]);
    }

    /// V4 6.12 has `Atomics` but throws on every access; these follow ECMA-262 §25.4.
    #[test]
    fn atomics() {
        check(&[
            (
                "(function () { var i = new Int32Array(new SharedArrayBuffer(8)); return [Atomics.add(i, 0, 5), Atomics.sub(i, 0, 2), Atomics.load(i, 0), Atomics.store(i, 1, 7.9), Atomics.exchange(i, 1, 3), Atomics.compareExchange(i, 1, 3, 9), Atomics.and(i, 1, 8), Atomics.or(i, 1, 1), Atomics.xor(i, 1, 3), i.join()].join() })()",
                "0,5,3,7,7,3,9,8,9,3,10",
            ),
            (
                "(function () { var i = new Uint8Array(4); return [Atomics.add(i, 0, 300), i[0]].join() })()",
                "0,44",
            ),
            (
                "Atomics.add(new Float32Array(4), 0, 1)",
                "TypeError: Type error",
            ),
            (
                "Atomics.add(new Uint8ClampedArray(4), 0, 1)",
                "TypeError: Type error",
            ),
            (
                "Atomics.load(new Int8Array(4), 4)",
                "RangeError: index out of range",
            ),
            (
                "[1, 2, 3, 4, 8].map(Atomics.isLockFree).join()",
                "true,false,false,true,false",
            ),
            (
                "Atomics.wait(new Int32Array(4), 0, 0, 0)",
                "TypeError: Type error",
            ),
            (
                "Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 0)",
                "TypeError: Atomics.wait cannot be called in this context",
            ),
            (
                "Atomics.notify(new Int32Array(new SharedArrayBuffer(4)), 0, 1)",
                "0",
            ),
            (
                "Atomics.wake(new Int32Array(new SharedArrayBuffer(4)), 0)",
                "0",
            ),
            (
                "new Int8Array(new SharedArrayBuffer(4)).buffer.constructor === SharedArrayBuffer",
                "true",
            ),
        ]);
    }
}
