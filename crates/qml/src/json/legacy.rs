//! The hand-written JSON code serde_json replaced, kept verbatim as the reference for the
//! equivalence tests.
#![allow(dead_code)]
use crate::builtins::color_name;
use crate::eval::{Interp, R, new_obj};
use crate::runtime::World;
use crate::value::{Value, num_to_string};
use std::rc::Rc;

/// `num_to_string` appended in place; integers (the common case) skip float formatting.
pub fn push_num(out: &mut String, n: f64) {
    if n.fract() == 0.0 && n.abs() < 9e15 {
        use std::fmt::Write;
        let _ = write!(out, "{}", n as i64);
    } else {
        out.push_str(&num_to_string(n))
    }
}

pub fn json_quote_into(s: &str, o: &mut String) {
    o.push('"');
    if s.bytes().all(|b| b >= 0x20 && b != b'"' && b != b'\\') {
        o.push_str(s);
        o.push('"');
        return;
    }
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            '\u{8}' => o.push_str("\\b"),
            '\u{c}' => o.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write;
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
}

pub fn json_str(
    it: &mut Interp,
    v: &Value,
    indent: &str,
    cur: &str,
    depth: usize,
) -> R<Option<String>> {
    let mut out = String::new();
    Ok(json_write(it, v, indent, cur, depth, &mut out)?.then_some(out))
}

/// Appends `v`'s JSON text to `out` (`cur`: the current line's indentation); false (nothing
/// written) for values JSON.stringify omits.
pub fn json_write(
    it: &mut Interp,
    v: &Value,
    indent: &str,
    cur: &str,
    depth: usize,
    out: &mut String,
) -> R<bool> {
    if depth > 256 {
        return it.throw("TypeError", "JSON.stringify: structure too deep");
    }
    match v {
        Value::Undefined | Value::Empty | Value::Native(_) | Value::Symbol(_) => return Ok(false),
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Num(n) => {
            if n.is_finite() {
                push_num(out, *n)
            } else {
                out.push_str("null")
            }
        }
        Value::Str(s) => json_quote_into(s, out),
        Value::Str16(u) => json_quote_into(&String::from_utf16_lossy(u), out),
        Value::Color(c) => json_quote_into(&color_name(c), out),
        Value::Item(_) => out.push_str("{}"),
        Value::Obj(o) => {
            if o.borrow().call.is_some() {
                return Ok(false);
            }
            let pretty = !indent.is_empty();
            let inner = if pretty {
                format!("{cur}{indent}")
            } else {
                String::new()
            };
            let b = o.borrow();
            if let Some(arr) = &b.array {
                if arr.is_empty() {
                    out.push_str("[]");
                    return Ok(true);
                }
                out.push('[');
                for (n, x) in arr.iter().enumerate() {
                    if n > 0 {
                        out.push(',');
                    }
                    if pretty {
                        out.push('\n');
                        out.push_str(&inner);
                    }
                    if !json_write(it, x, indent, &inner, depth + 1, out)? {
                        out.push_str("null");
                    }
                }
                if pretty {
                    out.push('\n');
                    out.push_str(cur);
                }
                out.push(']');
            } else {
                let start = out.len();
                out.push('{');
                let mut any = false;
                // Snapshot first: an object-literal getter runs JS (a cycle hits the depth limit).
                let es: Vec<(Rc<str>, Option<Value>)> = b
                    .keys
                    .iter()
                    .map(|k| (k.clone(), b.props.get(k).cloned()))
                    .collect();
                drop(b);
                for (k, x) in es {
                    let x = match x {
                        Some(x) => x,
                        None => it.get(v, &k)?,
                    };
                    let (k, x) = (&k, &x);
                    let mark = out.len();
                    if any {
                        out.push(',');
                    }
                    if pretty {
                        out.push('\n');
                        out.push_str(&inner);
                    }
                    json_quote_into(k, out);
                    out.push_str(if pretty { ": " } else { ":" });
                    if json_write(it, x, indent, &inner, depth + 1, out)? {
                        any = true;
                    } else {
                        out.truncate(mark);
                    }
                }
                if !any {
                    out.truncate(start);
                    out.push_str("{}");
                    return Ok(true);
                }
                if pretty {
                    out.push('\n');
                    out.push_str(cur);
                }
                out.push('}');
            }
        }
    }
    Ok(true)
}

/// Strict JSON text → JS value (`JSON.parse` without reviver).
pub fn parse_json(s: &str) -> Option<Value> {
    let mut p = JsonP {
        s: s.as_bytes(),
        i: 0,
    };
    let v = p.val(0)?;
    p.ws();
    (p.i == s.len()).then_some(v)
}

pub struct JsonP<'a> {
    s: &'a [u8],
    i: usize,
}

impl JsonP<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }
    fn val(&mut self, depth: usize) -> Option<Value> {
        if depth > 512 {
            return None;
        }
        self.ws();
        let c = *self.s.get(self.i)?;
        match c {
            b'{' => {
                self.i += 1;
                let o = new_obj();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Some(Value::Obj(o));
                }
                loop {
                    self.ws();
                    let Value::Str(k) = self.string()? else {
                        return None;
                    };
                    self.ws();
                    if self.s.get(self.i) != Some(&b':') {
                        return None;
                    }
                    self.i += 1;
                    let v = self.val(depth + 1)?;
                    crate::eval::set_own_rc(&o, k, v);
                    self.ws();
                    match self.s.get(self.i)? {
                        b',' => self.i += 1,
                        b'}' => {
                            self.i += 1;
                            return Some(Value::Obj(o));
                        }
                        _ => return None,
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Some(Value::array(v));
                }
                loop {
                    v.push(self.val(depth + 1)?);
                    self.ws();
                    match self.s.get(self.i)? {
                        b',' => self.i += 1,
                        b']' => {
                            self.i += 1;
                            return Some(Value::array(v));
                        }
                        _ => return None,
                    }
                }
            }
            b'"' => self.string(),
            b't' if self.s[self.i..].starts_with(b"true") => {
                self.i += 4;
                Some(Value::Bool(true))
            }
            b'f' if self.s[self.i..].starts_with(b"false") => {
                self.i += 5;
                Some(Value::Bool(false))
            }
            b'n' if self.s[self.i..].starts_with(b"null") => {
                self.i += 4;
                Some(Value::Null)
            }
            _ => {
                let st = self.i;
                while self.i < self.s.len()
                    && matches!(
                        self.s[self.i],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.i += 1;
                }
                std::str::from_utf8(&self.s[st..self.i])
                    .ok()?
                    .parse()
                    .ok()
                    .map(Value::Num)
            }
        }
    }
    fn string(&mut self) -> Option<Value> {
        if self.s.get(self.i) != Some(&b'"') {
            return None;
        }
        self.i += 1;
        let st = self.i;
        let end = self.s[st..]
            .iter()
            .position(|&c| c == b'"' || c == b'\\')
            .map(|n| st + n);
        if let Some(e) = end
            && self.s[e] == b'"'
            && let Ok(t) = std::str::from_utf8(&self.s[st..e])
        {
            self.i = e + 1;
            return Some(Value::str(t));
        }
        let mut u: Vec<u16> = Vec::new();
        loop {
            let c = *self.s.get(self.i)?;
            self.i += 1;
            match c {
                b'"' => return Some(from16(&u)),
                b'\\' => {
                    let e = *self.s.get(self.i)?;
                    self.i += 1;
                    match e {
                        b'n' => u.push(10),
                        b't' => u.push(9),
                        b'r' => u.push(13),
                        b'b' => u.push(8),
                        b'f' => u.push(12),
                        b'u' => {
                            let h = std::str::from_utf8(self.s.get(self.i..self.i + 4)?).ok()?;
                            u.push(u16::from_str_radix(h, 16).ok()?);
                            self.i += 4;
                        }
                        other => u.push(other as u16),
                    }
                }
                _ => {
                    let st = self.i - 1;
                    let len = match c {
                        0..=0x7f => 1,
                        0xc0..=0xdf => 2,
                        0xe0..=0xef => 3,
                        _ => 4,
                    };
                    let ch = std::str::from_utf8(self.s.get(st..st + len)?).ok()?;
                    u.extend(ch.encode_utf16());
                    self.i = st + len;
                }
            }
        }
    }
}

pub mod adapter {
    use super::*;
    pub fn json_write(w: &World, v: &Value, depth: usize, out: &mut String) {
        let pad = |n: usize| "    ".repeat(n);
        let obj = |pairs: Vec<(String, Value)>, out: &mut String| {
            // QJsonDocument::Indented: an empty container still breaks its line.
            if pairs.is_empty() {
                out.push_str("{\n");
                out.push_str(&pad(depth));
                out.push('}');
                return;
            }
            out.push_str("{\n");
            for (n, (k, v)) in pairs.iter().enumerate() {
                out.push_str(&pad(depth + 1));
                json_string(k, out);
                out.push_str(": ");
                json_write(w, v, depth + 1, out);
                out.push_str(if n + 1 < pairs.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(depth));
            out.push('}');
        };
        match v {
            Value::Item(c) if json_item(w, *c) => obj(json_props(w, *c), out),
            Value::Obj(o) => {
                let b = o.borrow();
                if let Some(a) = &b.array {
                    if a.is_empty() {
                        out.push_str("[\n");
                        out.push_str(&pad(depth));
                        out.push(']');
                        return;
                    }
                    out.push_str("[\n");
                    for (n, x) in a.iter().enumerate() {
                        out.push_str(&pad(depth + 1));
                        json_write(w, x, depth + 1, out);
                        out.push_str(if n + 1 < a.len() { ",\n" } else { "\n" });
                    }
                    out.push_str(&pad(depth));
                    out.push(']');
                } else if b.call.is_some() {
                    out.push_str("null");
                } else {
                    let mut pairs: Vec<(String, Value)> = b
                        .props
                        .iter()
                        .filter(|(_, v)| !matches!(v, Value::Undefined))
                        .map(|(k, v)| (k.to_string(), v.clone()))
                        .collect();
                    pairs.sort_by(|a, b| a.0.cmp(&b.0));
                    drop(b);
                    obj(pairs, out);
                }
            }
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Num(n) if n.is_finite() => out.push_str(&format!("{n}")),
            Value::Str(s) => json_string(s, out),
            Value::Color(_) => json_string(&v.to_js_string(), out),
            _ => out.push_str("null"),
        }
    }

    pub fn json_string(s: &str, out: &mut String) {
        out.push('"');
        for ch in s.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                '\u{8}' => out.push_str("\\b"),
                '\u{c}' => out.push_str("\\f"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
    }

    pub fn json_item(w: &World, i: usize) -> bool {
        w.items[i]
            .types
            .iter()
            .any(|t| t == "JsonAdapter" || t == "JsonObject")
    }

    /// The QML-declared properties of a JsonAdapter/JsonObject (not its C++ ones), sorted.
    pub fn json_props(w: &World, i: usize) -> Vec<(String, Value)> {
        let mut v: Vec<(String, Value)> = w.items[i]
            .props
            .iter()
            .filter(|(k, p)| {
                !k.starts_with("__")
                    && !k.starts_with('\x01')
                    && !k.contains('.')
                    && p.alias.is_none()
                    && crate::types::find_prop(&w.items[i].types, k).is_none()
                    && !matches!(&p.value, Value::Item(c) if !json_item(w, *c))
            })
            .map(|(k, p)| (k.to_string(), p.value.clone()))
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }
}

fn from16(u: &[u16]) -> Value {
    Value::from_utf16(u)
}
