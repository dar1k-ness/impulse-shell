//! JSON text <-> JS values on top of `serde_json`: `JSON.parse`/`JSON.stringify` (V8/V4
//! output, numbers via `ryu-js`) and the `QJsonDocument::Indented` text JsonAdapter writes.

use crate::eval::{Ex, Interp, R, new_obj, set_own_rc};
use crate::runtime::World;
use crate::value::Value;
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Error as _, SerializeMap, SerializeSeq, Serializer};
use serde::{Serialize, de::Error as _};
use serde_json::ser::{CompactFormatter, Formatter, PrettyFormatter};
use std::cell::RefCell;
use std::io::{self, Write};
use std::rc::Rc;

const MAX_PARSE_DEPTH: usize = 512;
/// V4 writes nesting as deep as its stack allows and throws `RangeError` beyond: the JS limit.
const MAX_STRINGIFY_DEPTH: usize = crate::eval::MAX_JS_DEPTH as usize;

// ---------- parse ----------

/// Strict JSON text → JS value (`JSON.parse` without reviver).
pub fn parse(s: &str) -> Result<Value, serde_json::Error> {
    parse_str(s).or_else(|e| {
        let t = v4_dot_numbers(s);
        if has_lone_surrogate_escape(&t) {
            Lone { s: &t, i: 0 }.text().ok_or(e)
        } else if t != s {
            parse_str(&t)
        } else {
            Err(e)
        }
    })
}

/// QJSEngine (V4) also reads numbers with digits on one side of the point only (`.5`, `-.5`,
/// `1.`, `1.e5`; not `.` or `.e5`), which JSON forbids; serde_json gets them with that `0`.
fn v4_dot_numbers(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let (mut i, mut last, mut in_str) = (0, 0, false);
    while i < b.len() {
        match b[i] {
            b'\\' if in_str => i += 1,
            b'"' => in_str = !in_str,
            b'.' if !in_str => {
                let before = i > 0 && b[i - 1].is_ascii_digit();
                let after = b.get(i + 1).is_some_and(u8::is_ascii_digit);
                if before != after {
                    out.push_str(&s[last..i]);
                    out.push_str(if before { ".0" } else { "0." });
                    last = i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out.push_str(&s[last..]);
    out
}

fn parse_str(s: &str) -> Result<Value, serde_json::Error> {
    let mut de = serde_json::Deserializer::from_str(s);
    de.disable_recursion_limit();
    let v = Val(0).deserialize(&mut de)?;
    de.end()?;
    Ok(v)
}

/// The `\uXXXX` escape at `b[i..]`.
fn hex_escape(b: &[u8], i: usize) -> Option<u16> {
    let h = b.get(i..i + 6)?;
    if h[0] != b'\\' || h[1] != b'u' {
        return None;
    }
    u16::from_str_radix(std::str::from_utf8(&h[2..]).ok()?, 16).ok()
}

/// Whether a string in JSON text `s` escapes an unpaired surrogate (`"\ud800"`), which
/// serde_json's `str` mode rejects.
fn has_lone_surrogate_escape(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' {
            i += 1;
            continue;
        }
        match hex_escape(b, i) {
            Some(0xd800..=0xdbff) if matches!(hex_escape(b, i + 6), Some(0xdc00..=0xdfff)) => {
                i += 12
            }
            Some(0xd800..=0xdfff) => return true,
            Some(_) => i += 6,
            None => i += 2,
        }
    }
    false
}

/// JSON.parse (ECMA-262 §25.5.1) of a text whose strings escape unpaired surrogates: the
/// same JSON grammar (RFC 8259) as serde_json reads, strings as UTF-16 code units so each
/// lone surrogate stays one. A property name keeps its text with U+FFFD for one.
struct Lone<'a> {
    s: &'a str,
    i: usize,
}

impl Lone<'_> {
    fn text(mut self) -> Option<Value> {
        let v = self.value(0)?;
        self.ws();
        (self.i == self.s.len()).then_some(v)
    }

    fn peek(&self) -> Option<u8> {
        self.s.as_bytes().get(self.i).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        let hit = self.peek() == Some(c);
        self.i += hit as usize;
        hit
    }

    fn word(&mut self, w: &str, v: Value) -> Option<Value> {
        self.s[self.i..].starts_with(w).then(|| {
            self.i += w.len();
            v
        })
    }

    fn value(&mut self, depth: usize) -> Option<Value> {
        if depth > MAX_PARSE_DEPTH {
            return None;
        }
        self.ws();
        match self.peek()? {
            b'{' => {
                self.i += 1;
                let o = new_obj();
                if self.eat(b'}') {
                    return Some(Value::Obj(o));
                }
                loop {
                    self.ws();
                    let k = self.string()?;
                    if !self.eat(b':') {
                        return None;
                    }
                    let v = self.value(depth + 1)?;
                    set_own_rc(&o, Rc::from(k.to_js_string()), v);
                    if !self.eat(b',') {
                        return self.eat(b'}').then_some(Value::Obj(o));
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut a = Vec::new();
                if self.eat(b']') {
                    return Some(Value::array(a));
                }
                loop {
                    a.push(self.value(depth + 1)?);
                    if !self.eat(b',') {
                        return self.eat(b']').then_some(Value::array(a));
                    }
                }
            }
            b'"' => self.string(),
            b't' => self.word("true", Value::Bool(true)),
            b'f' => self.word("false", Value::Bool(false)),
            b'n' => self.word("null", Value::Null),
            _ => self.number(),
        }
    }

    fn string(&mut self) -> Option<Value> {
        if self.peek() != Some(b'"') {
            return None;
        }
        self.i += 1;
        let b = self.s.as_bytes();
        let mut u = Vec::new();
        loop {
            match *b.get(self.i)? {
                b'"' => {
                    self.i += 1;
                    return Some(Value::from_utf16(&u));
                }
                0..0x20 => return None,
                b'\\' => {
                    let c = match *b.get(self.i + 1)? {
                        b'"' => b'"',
                        b'\\' => b'\\',
                        b'/' => b'/',
                        b'b' => 8,
                        b'f' => 0xc,
                        b'n' => b'\n',
                        b'r' => b'\r',
                        b't' => b'\t',
                        b'u' => {
                            u.push(hex_escape(b, self.i)?);
                            self.i += 6;
                            continue;
                        }
                        _ => return None,
                    };
                    u.push(c as u16);
                    self.i += 2;
                }
                _ => {
                    let c = self.s[self.i..].chars().next()?;
                    u.extend(c.encode_utf16(&mut [0; 2]).iter());
                    self.i += c.len_utf8();
                }
            }
        }
    }

    fn number(&mut self) -> Option<Value> {
        let b = self.s.as_bytes();
        let start = self.i;
        let digits = |i: &mut usize| {
            let from = *i;
            while b.get(*i).is_some_and(u8::is_ascii_digit) {
                *i += 1;
            }
            *i > from
        };
        let mut i = self.i + (b.get(self.i) == Some(&b'-')) as usize;
        match b.get(i)? {
            b'0' => i += 1,
            b'1'..=b'9' => _ = digits(&mut i),
            _ => return None,
        }
        if b.get(i) == Some(&b'.') {
            i += 1;
            if !digits(&mut i) {
                return None;
            }
        }
        if matches!(b.get(i), Some(b'e' | b'E')) {
            i += 1;
            if matches!(b.get(i), Some(b'+' | b'-')) {
                i += 1;
            }
            if !digits(&mut i) {
                return None;
            }
        }
        self.i = i;
        v4_number(&self.s[start..i]).map(Value::Num)
    }
}

/// Deserializes one JSON value at nesting depth `.0` straight into a [`Value`].
struct Val(usize);

impl<'de> DeserializeSeed<'de> for Val {
    type Value = Value;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        if self.0 > MAX_PARSE_DEPTH {
            return Err(D::Error::custom("nesting too deep"));
        }
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Val {
    type Value = Value;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a JSON value")
    }
    fn visit_bool<E>(self, b: bool) -> Result<Value, E> {
        Ok(Value::Bool(b))
    }
    fn visit_i64<E>(self, n: i64) -> Result<Value, E> {
        Ok(Value::Num(n as f64))
    }
    fn visit_u64<E>(self, n: u64) -> Result<Value, E> {
        Ok(Value::Num(n as f64))
    }
    fn visit_f64<E>(self, n: f64) -> Result<Value, E> {
        Ok(Value::Num(n))
    }
    fn visit_str<E>(self, s: &str) -> Result<Value, E> {
        Ok(Value::str(s))
    }
    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
        let mut v = Vec::new();
        while let Some(x) = a.next_element_seed(Val(self.0 + 1))? {
            v.push(x);
        }
        Ok(Value::array(v))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
        if std::any::type_name::<A>().ends_with("::NumberDeserializer") {
            // `arbitrary_precision`: a number that is not a u64/i64, as its JSON text.
            a.next_key::<de::IgnoredAny>()?;
            let text: String = a.next_value()?;
            return v4_number(&text)
                .map(Value::Num)
                .ok_or_else(|| A::Error::custom("number out of range"));
        }
        let o = new_obj();
        while let Some(k) = a.next_key_seed(Key)? {
            let v = a.next_value_seed(Val(self.0 + 1))?;
            set_own_rc(&o, k, v);
        }
        Ok(Value::Obj(o))
    }
}

/// A JSON number as QJSEngine (V4) reads it: correctly rounded, but a SyntaxError when it
/// rounds to ±Infinity or a non-zero mantissa rounds to 0 (strtod's ERANGE), unlike V8.
fn v4_number(text: &str) -> Option<f64> {
    let n: f64 = text.parse().ok()?;
    let mantissa = text.split(['e', 'E']).next().unwrap_or("");
    let underflow = n == 0.0 && mantissa.bytes().any(|b| matches!(b, b'1'..=b'9'));
    (n.is_finite() && !underflow).then_some(n)
}

struct Key;

impl<'de> DeserializeSeed<'de> for Key {
    type Value = Rc<str>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Rc<str>, D::Error> {
        d.deserialize_str(self)
    }
}

impl<'de> Visitor<'de> for Key {
    type Value = Rc<str>;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a key")
    }
    fn visit_str<E: de::Error>(self, s: &str) -> Result<Rc<str>, E> {
        Ok(Rc::from(s))
    }
}

// ---------- JSON.stringify ----------

/// QuoteJSONString (ECMA-262 §25.5.2.3) of a String with an unpaired surrogate: as serde_json
/// writes a well-formed one (short escapes, other controls `\u00XX`), each unpaired
/// surrogate as a lower-case `\uXXXX` escape (well-formed stringify).
fn quote_utf16(u: &[u16]) -> String {
    use std::fmt::Write;
    let mut o = String::with_capacity(u.len() + 2);
    o.push('"');
    for c in char::decode_utf16(u.iter().copied()) {
        match c {
            Ok('"') => o.push_str("\\\""),
            Ok('\\') => o.push_str("\\\\"),
            Ok('\u{8}') => o.push_str("\\b"),
            Ok('\u{c}') => o.push_str("\\f"),
            Ok('\n') => o.push_str("\\n"),
            Ok('\r') => o.push_str("\\r"),
            Ok('\t') => o.push_str("\\t"),
            Ok(c) if (c as u32) < 0x20 => _ = write!(o, "\\u{:04x}", c as u32),
            Ok(c) => o.push(c),
            Err(e) => _ = write!(o, "\\u{:04x}", e.unpaired_surrogate()),
        }
    }
    o.push('"');
    o
}

/// Numbers as ECMAScript `Number::toString` (`1e+21`, `0.1`, `-0` → `0`).
struct V8Numbers<F>(F);

macro_rules! delegate {
    ($($m:ident($($a:ident: $t:ty),*)),* $(,)?) => {$(
        #[inline]
        fn $m<W: ?Sized + Write>(&mut self, w: &mut W $(, $a: $t)*) -> io::Result<()> {
            self.0.$m(w $(, $a)*)
        }
    )*};
}

impl<F: Formatter> Formatter for V8Numbers<F> {
    fn write_f64<W: ?Sized + Write>(&mut self, w: &mut W, n: f64) -> io::Result<()> {
        // Safe integers print as their digits (-0 as "0"); itoa is cheaper than ryu for them.
        if n.fract() == 0.0 && n.abs() < 9007199254740992.0 {
            return self.0.write_i64(w, n as i64);
        }
        w.write_all(ryu_js::Buffer::new().format(n).as_bytes())
    }
    delegate!(
        begin_array(),
        end_array(),
        begin_array_value(first: bool),
        end_array_value(),
        begin_object(),
        end_object(),
        begin_object_key(first: bool),
        begin_object_value(),
        end_object_value(),
    );
}

struct Cx<'i> {
    it: &'i mut Interp,
    err: Option<Ex>,
    /// Objects/arrays being serialized, outermost first (the cycle check).
    open: Vec<usize>,
    /// The `replacer` function of `JSON.stringify(value, replacer)`.
    replacer: Option<Value>,
    /// The PropertyList of an array `replacer`: the keys every object serializes.
    keys: Option<Rc<[Rc<str>]>>,
}

struct Js<'a, 'i> {
    cx: &'a RefCell<Cx<'i>>,
    v: &'a Value,
    depth: usize,
}

/// Values `JSON.stringify` drops from objects (and writes as `null` in arrays).
fn omitted(v: &Value) -> bool {
    match v {
        Value::Undefined | Value::Native(_) | Value::Symbol(_) => true,
        Value::Obj(o) => o.borrow().call.is_some(),
        _ => false,
    }
}

impl<'i> Js<'_, 'i> {
    fn fail<E: serde::ser::Error>(&self, ex: Ex) -> E {
        self.cx.borrow_mut().err = Some(ex);
        E::custom("JS exception")
    }
    /// `value.toJSON(key)` when `value` has a callable `toJSON` (a Date), else `value`.
    fn to_json<E: serde::ser::Error>(&self, key: &str, v: Value) -> Result<Value, E> {
        let r = {
            let cx = &mut *self.cx.borrow_mut();
            replace(cx.it, cx.replacer.as_ref(), self.v, key, v)
        };
        r.map_err(|ex| self.fail(ex))
    }
    fn child<'b>(&'b self, v: &'b Value) -> Js<'b, 'i> {
        Js {
            cx: self.cx,
            v,
            depth: self.depth + 1,
        }
    }
}

impl Serialize for Js<'_, '_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.depth > MAX_STRINGIFY_DEPTH {
            let ex = self.cx.borrow().it.stack_overflow::<()>();
            return Err(self.fail(ex.unwrap_err()));
        }
        crate::eval::on_js_stack(|| self.value(s))
    }
}

impl Js<'_, '_> {
    fn value<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.v {
            Value::Undefined | Value::Empty | Value::Native(_) | Value::Symbol(_) | Value::Null => {
                s.serialize_unit()
            }
            Value::Bool(b) => s.serialize_bool(*b),
            Value::Num(n) => s.serialize_f64(*n),
            Value::Str(t) => s.serialize_str(t),
            Value::Str16(u) => serde_json::value::RawValue::from_string(quote_utf16(u))
                .map_err(S::Error::custom)?
                .serialize(s),
            // A color is an object with the properties of its type.
            Value::Color(c) => {
                let mut m = s.serialize_map(None)?;
                for k in crate::valuetypes::COLOR_PROPERTIES {
                    let v = crate::builtins::color_prop(c, k);
                    m.serialize_entry(k, &self.child(&v))?;
                }
                m.end()
            }
            // A QObject wrapper's enumerable own properties (QObjectWrapper has no JSON form
            // of its own); its signals and methods are functions, which JSON omits.
            Value::Item(i) => {
                // Item ids from the top of the address space cannot be an object's address.
                let id = usize::MAX - *i;
                if self.cx.borrow().open.contains(&id) {
                    let ex = self
                        .cx
                        .borrow()
                        .it
                        .throw::<()>("TypeError", "Cannot convert circular structure to JSON");
                    return Err(self.fail(ex.unwrap_err()));
                }
                self.cx.borrow_mut().open.push(id);
                let r = match self.property_list() {
                    Some(keys) => self.listed(&keys, s),
                    None => {
                        let v = self.v;
                        let keys: Vec<Rc<str>> = self
                            .with_it(|it| it.keys(v))?
                            .into_iter()
                            .map(Rc::from)
                            .collect();
                        self.listed(&keys, s)
                    }
                };
                self.cx.borrow_mut().open.pop();
                r
            }
            // SerializeJSONProperty step 4 (ECMA-262 §25.5.2.2): a Number or String object is
            // its ToNumber or ToString, a Boolean object its [[BooleanData]].
            Value::Obj(o) if let Some(p) = crate::value::wrapped(o) => {
                let p = {
                    let cx = &mut *self.cx.borrow_mut();
                    match p {
                        Value::Num(_) => cx.it.to_numeric(self.v).map(Value::Num),
                        Value::Str(_) | Value::Str16(_) => cx.it.to_str_value(self.v),
                        p => Ok(p),
                    }
                };
                let p = p.map_err(|ex| self.fail(ex))?;
                Js {
                    cx: self.cx,
                    v: &p,
                    depth: self.depth,
                }
                .value(s)
            }
            Value::Obj(o) => {
                let id = Rc::as_ptr(o) as *const () as usize;
                if self.cx.borrow().open.contains(&id) {
                    let ex = self
                        .cx
                        .borrow()
                        .it
                        .throw::<()>("TypeError", "Cannot convert circular structure to JSON");
                    return Err(self.fail(ex.unwrap_err()));
                }
                self.cx.borrow_mut().open.push(id);
                let r = self.object(o, s);
                self.cx.borrow_mut().open.pop();
                r
            }
        }
    }

    fn object<S: Serializer>(&self, o: &crate::value::Obj, s: S) -> Result<S::Ok, S::Error> {
        if o.borrow().is_proxy() {
            return self.proxy(s);
        }
        let b = o.borrow();
        if let Some(arr) = &b.array {
            let arr = arr.clone();
            let holes = b.holes.clone();
            let len = b.array_length();
            drop(b);
            let ov = Value::Obj(o.clone());
            let mut q = s.serialize_seq(Some(len))?;
            for i in 0..len {
                let k = i.to_string();
                // SerializeJSONArray reads a hole, and an element after the stored ones,
                // through [[Get]] (ECMA-262 §25.5.2.6): an index accessor, an ordinary
                // property or an inherited element.
                let x = match arr.get(i) {
                    Some(x) if !holes.as_ref().is_some_and(|h| h.contains(i)) => x.clone(),
                    _ => self.with_it(|it| it.get(&ov, &k))?,
                };
                let x = self.to_json(&k, x)?;
                if omitted(&x) {
                    q.serialize_element(&())?;
                } else {
                    q.serialize_element(&self.child(&x))?;
                }
            }
            q.end()
        } else if let Some(keys) = self.property_list() {
            drop(b);
            self.listed(&keys, s)
        } else {
            // Snapshot first: an object-literal getter runs JS. A typed array's elements are
            // its first own keys (ECMA-262 §10.4.5.7).
            let elems = crate::typedarray::typed(o)
                .map(|t| t.values())
                .unwrap_or_default();
            let mut es: Vec<(Rc<str>, Option<Value>)> = elems
                .into_iter()
                .enumerate()
                .map(|(i, v)| (Rc::from(i.to_string()), Some(v)))
                .chain(b.keys.iter().map(|k| (k.clone(), b.props.get(k).cloned())))
                .collect();
            drop(b);
            // A value type lists what script defined on it after its own properties.
            if let Some(m) = crate::valuetypes::members(o) {
                let ks = self.with_it(|it| it.keys(&Value::Obj(m)))?;
                es.extend(ks.into_iter().map(|k| (Rc::from(k), None)));
            }
            let mut m = s.serialize_map(None)?;
            for (k, x) in es {
                let x = match x {
                    Some(x) => x,
                    None => {
                        let r = self.cx.borrow_mut().it.get(self.v, &k);
                        r.map_err(|ex| self.fail::<S::Error>(ex))?
                    }
                };
                let x = self.to_json(&k, x)?;
                if !omitted(&x) {
                    m.serialize_entry(&*k, &self.child(&x))?;
                }
            }
            m.end()
        }
    }
}

impl Js<'_, '_> {
    /// `f` on the interpreter, its exception kept for `JSON.stringify` to rethrow.
    fn with_it<T, E: serde::ser::Error>(
        &self,
        f: impl FnOnce(&mut Interp) -> R<T>,
    ) -> Result<T, E> {
        let r = f(&mut *self.cx.borrow_mut().it);
        r.map_err(|ex| self.fail(ex))
    }

    /// SerializeJSONArray / SerializeJSONObject (ECMA-262 §25.5.2.5-6) of a proxy: through its
    /// IsArray, [[Get]] and enumerable own keys.
    fn proxy<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let v = self.v;
        if self.with_it(|it| crate::props::is_array(it, v))? {
            let len = self.with_it(|it| {
                let n = it.get(v, "length")?;
                let n = it.to_numeric(&n)?;
                if n > 0.0 {
                    crate::eval::array_len(it, n.trunc().min(4294967295.0))
                } else {
                    Ok(0)
                }
            })?;
            let mut q = s.serialize_seq(Some(len))?;
            for i in 0..len {
                let k = i.to_string();
                let x = self.with_it(|it| it.get(v, &k))?;
                let x = self.to_json(&k, x)?;
                if omitted(&x) {
                    q.serialize_element(&())?;
                } else {
                    q.serialize_element(&self.child(&x))?;
                }
            }
            return q.end();
        }
        if let Some(keys) = self.property_list() {
            return self.listed(&keys, s);
        }
        let keys = self.with_it(|it| it.keys(v))?;
        let mut m = s.serialize_map(None)?;
        for k in keys {
            let x = self.with_it(|it| it.get(v, &k))?;
            let x = self.to_json(&k, x)?;
            if !omitted(&x) {
                m.serialize_entry(&*k, &self.child(&x))?;
            }
        }
        m.end()
    }
}

impl Js<'_, '_> {
    /// The PropertyList of an array replacer, if any.
    fn property_list(&self) -> Option<Rc<[Rc<str>]>> {
        self.cx.borrow().keys.clone()
    }

    /// SerializeJSONObject with a PropertyList (ECMA-262 §25.5.2.5 step 5): exactly the
    /// listed keys, each read with [[Get]].
    fn listed<S: Serializer>(&self, keys: &[Rc<str>], s: S) -> Result<S::Ok, S::Error> {
        let v = self.v;
        let mut m = s.serialize_map(None)?;
        for k in keys {
            let x = self.with_it(|it| it.get(v, k))?;
            let x = self.to_json(k, x)?;
            if !omitted(&x) {
                m.serialize_entry(&**k, &self.child(&x))?;
            }
        }
        m.end()
    }
}

/// The PropertyList of `JSON.stringify(value, replacer)` for an array `replacer` (ECMA-262
/// §25.5.2.1 step 4.b): its String and Number elements (and their wrapper objects) as
/// strings, in order, without repeats; `None` when `replacer` is not an array.
pub fn property_list(it: &mut Interp, replacer: &Value) -> R<Option<Vec<Rc<str>>>> {
    if !crate::props::is_array(it, replacer)? {
        return Ok(None);
    }
    let len = it.get(replacer, "length")?;
    let len = it.to_numeric(&len)?;
    let len = if len > 0.0 {
        len.trunc().min(9007199254740991.0)
    } else {
        0.0
    };
    let mut keys: Vec<Rc<str>> = Vec::new();
    let mut k = 0.0;
    while k < len {
        let v = it.get(replacer, &crate::value::num_to_string(k))?;
        let item = match &v {
            Value::Str(s) => Some(s.clone()),
            Value::Num(_) => Some(Rc::from(it.to_string(&v)?)),
            Value::Obj(o)
                if matches!(
                    crate::value::wrapped(o),
                    Some(Value::Str(_) | Value::Num(_))
                ) =>
            {
                Some(Rc::from(it.to_string(&v)?))
            }
            _ => None,
        };
        if let Some(item) = item
            && !keys.contains(&item)
        {
            keys.push(item);
        }
        k += 1.0;
    }
    Ok(Some(keys))
}

/// `JSON.parse(text, reviver)` after parsing `text` to `unfiltered` (ECMA-262 §25.5.1 step
/// 11): InternalizeJSONProperty of a fresh root holder's `""`.
pub fn revive(it: &mut Interp, unfiltered: Value, reviver: &Value) -> R<Value> {
    let root = new_obj();
    set_own_rc(&root, Rc::from(""), unfiltered);
    internalize(it, &Value::Obj(root), "", reviver)
}

/// InternalizeJSONProperty (ECMA-262 §25.5.1.1): revives `holder[name]` bottom-up, its
/// elements or enumerable own string keys first (listed before any reviver runs, read with
/// [[Get]]), deleting those the reviver maps to `undefined`.
fn internalize(it: &mut Interp, holder: &Value, name: &str, reviver: &Value) -> R<Value> {
    let val = it.get(holder, name)?;
    if let Value::Obj(o) = &val {
        let keys: Vec<String> = if crate::props::is_array(it, &val)? {
            let len = it.get(&val, "length")?;
            let len = it.to_numeric(&len)?;
            let len = if len > 0.0 {
                len.trunc().min(9007199254740991.0)
            } else {
                0.0
            };
            let mut ks = Vec::new();
            let mut i = 0.0;
            while i < len {
                ks.push(crate::value::num_to_string(i));
                i += 1.0;
            }
            ks
        } else {
            it.keys(&val)?
        };
        for k in keys {
            let e = {
                // A reviver can graft new objects onto what is still to be walked.
                let _frame = it.enter()?;
                crate::eval::on_js_stack(|| internalize(it, &val, &k, reviver))?
            };
            let pk = crate::eval::PropKey::Str(k);
            if matches!(e, Value::Undefined) {
                crate::props::delete_pk(it, o, &pk)?;
            } else {
                let d = crate::props::Desc::data(e, true, true, true);
                crate::props::define_pk(it, o, &pk, &d)?;
            }
        }
    }
    it.call(reviver, holder.clone(), &[Value::str(name), val])
}

/// SerializeJSONProperty's value of `holder[key]` (already read as `v`): `toJSON`, then the
/// replacer function, called on `holder`.
fn replace(
    it: &mut Interp,
    replacer: Option<&Value>,
    holder: &Value,
    key: &str,
    v: Value,
) -> R<Value> {
    let v = to_json(it, key, v)?;
    match replacer {
        Some(f) => it.call(f, holder.clone(), &[Value::str(key), v]),
        None => Ok(v),
    }
}

fn to_json(it: &mut Interp, key: &str, v: Value) -> R<Value> {
    // A `url` (a QV4::VariantObject holding a QUrl) serializes as its string.
    if let Some(u) = crate::valuetypes::url_of(&v) {
        return Ok(Value::str(&u));
    }
    if !matches!(&v, Value::Obj(o) if o.borrow().call.is_none()) {
        return Ok(v);
    }
    let f = it.get(&v, "toJSON")?;
    if crate::eval::callable(&f).is_none() {
        return Ok(v);
    }
    it.call(&f, v, &[Value::str(key)])
}

/// `JSON.stringify(v, null, indent)`; `None` for values it maps to `undefined`.
pub fn stringify(it: &mut Interp, v: &Value, indent: &str) -> R<Option<String>> {
    stringify_with(it, v, None, None, indent)
}

/// `JSON.stringify(v, replacer, indent)` with a replacer function (called on each holder with
/// the key and value) or the PropertyList `keys` of an array replacer (ECMA-262 §25.5.2.1;
/// V4 ignores the array).
pub fn stringify_with(
    it: &mut Interp,
    v: &Value,
    replacer: Option<Value>,
    keys: Option<Vec<Rc<str>>>,
    indent: &str,
) -> R<Option<String>> {
    let v = &match &replacer {
        Some(f) => {
            let holder = crate::eval::new_obj();
            crate::eval::set_own(&holder, "", v.clone());
            replace(it, Some(f), &Value::Obj(holder), "", v.clone())?
        }
        None => to_json(it, "", v.clone())?,
    };
    if omitted(v) {
        return Ok(None);
    }
    let cx = RefCell::new(Cx {
        it,
        err: None,
        open: Vec::new(),
        replacer,
        keys: keys.map(Rc::from),
    });
    let js = Js {
        cx: &cx,
        v,
        depth: 0,
    };
    let mut out = Vec::new();
    let r = if indent.is_empty() {
        js.serialize(&mut serde_json::Serializer::with_formatter(
            &mut out,
            V8Numbers(CompactFormatter),
        ))
    } else {
        let f = V8Numbers(PrettyFormatter::with_indent(indent.as_bytes()));
        js.serialize(&mut serde_json::Serializer::with_formatter(&mut out, f))
    };
    if let Some(ex) = cx.into_inner().err {
        return Err(ex);
    }
    r.expect("JSON.stringify writes to memory");
    Ok(Some(
        String::from_utf8(out).expect("serde_json writes UTF-8"),
    ))
}

// ---------- JsonAdapter (QJsonDocument::Indented) ----------

/// `QJsonDocument::Indented`: 4-space indent, and an empty container still breaks its line.
struct QtIndented {
    depth: usize,
}

impl QtIndented {
    fn pad<W: ?Sized + Write>(&self, w: &mut W) -> io::Result<()> {
        for _ in 0..self.depth {
            w.write_all(b"    ")?;
        }
        Ok(())
    }
    fn open<W: ?Sized + Write>(&mut self, w: &mut W, c: &[u8]) -> io::Result<()> {
        self.depth += 1;
        w.write_all(c)
    }
    fn close<W: ?Sized + Write>(&mut self, w: &mut W, c: &[u8]) -> io::Result<()> {
        self.depth -= 1;
        w.write_all(b"\n")?;
        self.pad(w)?;
        w.write_all(c)
    }
    fn item<W: ?Sized + Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        w.write_all(if first { b"\n" } else { b",\n" })?;
        self.pad(w)
    }
}

impl Formatter for QtIndented {
    fn write_f64<W: ?Sized + Write>(&mut self, w: &mut W, n: f64) -> io::Result<()> {
        write!(w, "{n}")
    }
    fn begin_array<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.open(w, b"[")
    }
    fn end_array<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.close(w, b"]")
    }
    fn begin_array_value<W: ?Sized + Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        self.item(w, first)
    }
    fn begin_object<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.open(w, b"{")
    }
    fn end_object<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.close(w, b"}")
    }
    fn begin_object_key<W: ?Sized + Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        self.item(w, first)
    }
    fn begin_object_value<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        w.write_all(b": ")
    }
}

fn adapter_item(w: &World, i: usize) -> bool {
    w.items[i]
        .types
        .iter()
        .any(|t| t == "JsonAdapter" || t == "JsonObject")
}

/// The QML-declared properties of a JsonAdapter/JsonObject (not its C++ ones), sorted.
fn adapter_props(w: &World, i: usize) -> Vec<(String, Value)> {
    let mut v: Vec<(String, Value)> = w.items[i]
        .props
        .iter()
        .filter(|(k, p)| {
            !k.starts_with("__")
                && !k.starts_with('\x01')
                && !k.contains('.')
                && p.alias.is_none()
                && crate::types::find_prop(&w.items[i].types, k).is_none()
                && !matches!(&p.value, Value::Item(c) if !adapter_item(w, *c))
        })
        .map(|(k, p)| (k.to_string(), p.value.clone()))
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

struct Qt<'a> {
    w: &'a World,
    v: &'a Value,
}

fn qt_map<S: Serializer>(w: &World, pairs: &[(String, Value)], s: S) -> Result<S::Ok, S::Error> {
    let mut m = s.serialize_map(Some(pairs.len()))?;
    for (k, v) in pairs {
        m.serialize_entry(k, &Qt { w, v })?;
    }
    m.end()
}

impl Serialize for Qt<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.v {
            Value::Item(c) if adapter_item(self.w, *c) => {
                qt_map(self.w, &adapter_props(self.w, *c), s)
            }
            Value::Obj(o) if let Some(v) = crate::value::wrapped(o) => {
                Qt { w: self.w, v: &v }.serialize(s)
            }
            Value::Obj(o) => {
                let b = o.borrow();
                if let Some(a) = &b.array {
                    let mut q = s.serialize_seq(Some(a.len()))?;
                    for v in a {
                        q.serialize_element(&Qt { w: self.w, v })?;
                    }
                    q.end()
                } else if b.call.is_some() {
                    s.serialize_unit()
                } else {
                    // QJsonObject is an ordered map.
                    let mut pairs: Vec<(String, Value)> = b
                        .props
                        .iter()
                        .filter(|(_, v)| !matches!(v, Value::Undefined))
                        .map(|(k, v)| (k.to_string(), v.clone()))
                        .collect();
                    pairs.sort_by(|a, b| a.0.cmp(&b.0));
                    drop(b);
                    qt_map(self.w, &pairs, s)
                }
            }
            Value::Bool(b) => s.serialize_bool(*b),
            Value::Num(n) => s.serialize_f64(*n),
            Value::Str(t) => s.serialize_str(t),
            Value::Color(_) => s.serialize_str(&self.v.to_js_string()),
            _ => s.serialize_unit(),
        }
    }
}

/// A JsonAdapter's file text, as Quickshell writes it (`QJsonDocument::Indented`).
pub fn adapter_text(w: &World, adapter: usize) -> String {
    let mut out = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut out, QtIndented { depth: 0 });
    Qt {
        w,
        v: &Value::Item(adapter),
    }
    .serialize(&mut ser)
    .expect("JsonAdapter text is written to memory");
    out.push(b'\n');
    String::from_utf8(out).expect("serde_json writes UTF-8")
}

#[cfg(test)]
mod legacy;
#[cfg(test)]
mod tests;
