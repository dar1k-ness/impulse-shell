//! The jq (1.8) behaviour the scripts rely on: number-literal preserving parse, the default
//! 2-space pretty printer and string escaping, `-r` output, path reads and `.a.b = $x`.

#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    /// The literal as jq prints it (decNumber canonical form).
    Num(String),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    fn kind(&self) -> &'static str {
        match self {
            J::Null => "null",
            J::Bool(_) => "boolean",
            J::Num(_) => "number",
            J::Str(_) => "string",
            J::Arr(_) => "array",
            J::Obj(_) => "object",
        }
    }

    pub fn obj_set(o: &mut Vec<(String, J)>, k: &str, v: J) {
        match o.iter_mut().find(|(n, _)| n == k) {
            Some(e) => e.1 = v,
            None => o.push((k.to_string(), v)),
        }
    }
}

/// `.k` (`Err` is jq's message).
pub fn field(v: &J, k: &str) -> Result<J, String> {
    match v {
        J::Null => Ok(J::Null),
        J::Obj(o) => Ok(o
            .iter()
            .find(|(n, _)| n == k)
            .map_or(J::Null, |e| e.1.clone())),
        v => Err(format!("Cannot index {} with string \"{k}\"", v.kind())),
    }
}

/// `.[i]`.
pub fn index(v: &J, i: usize) -> Result<J, String> {
    match v {
        J::Null => Ok(J::Null),
        J::Arr(a) => Ok(a.get(i).cloned().unwrap_or(J::Null)),
        v => Err(format!("Cannot index {} with number", v.kind())),
    }
}

/// `.a.b = x`.
pub fn set_path(v: &J, a: &str, b: &str, x: J) -> Result<J, String> {
    let mut root = match v {
        J::Null => Vec::new(),
        J::Obj(o) => o.clone(),
        v => return Err(format!("Cannot index {} with string \"{a}\"", v.kind())),
    };
    let mut inner = match root.iter().find(|(n, _)| n == a).map(|e| &e.1) {
        None | Some(J::Null) => Vec::new(),
        Some(J::Obj(o)) => o.clone(),
        Some(v) => return Err(format!("Cannot index {} with string \"{b}\"", v.kind())),
    };
    J::obj_set(&mut inner, b, x);
    J::obj_set(&mut root, a, J::Obj(inner));
    Ok(J::Obj(root))
}

pub fn quote_into(s: &str, o: &mut String) {
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            '\u{8}' => o.push_str("\\b"),
            '\u{c}' => o.push_str("\\f"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
}

pub fn quote(s: &str) -> String {
    let mut o = String::new();
    quote_into(s, &mut o);
    o
}

/// jq's default output (indent 2, no trailing newline).
pub fn pretty(v: &J) -> String {
    let mut o = String::new();
    pretty_into(v, 0, &mut o);
    o
}

fn pretty_into(v: &J, depth: usize, o: &mut String) {
    let pad = |o: &mut String, d: usize| o.extend(std::iter::repeat_n(' ', d * 2));
    match v {
        J::Null => o.push_str("null"),
        J::Bool(b) => o.push_str(if *b { "true" } else { "false" }),
        J::Num(n) => o.push_str(n),
        J::Str(s) => quote_into(s, o),
        J::Arr(a) if a.is_empty() => o.push_str("[]"),
        J::Obj(m) if m.is_empty() => o.push_str("{}"),
        J::Arr(a) => {
            o.push_str("[\n");
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    o.push_str(",\n");
                }
                pad(o, depth + 1);
                pretty_into(x, depth + 1, o);
            }
            o.push('\n');
            pad(o, depth);
            o.push(']');
        }
        J::Obj(m) => {
            o.push_str("{\n");
            for (i, (k, x)) in m.iter().enumerate() {
                if i > 0 {
                    o.push_str(",\n");
                }
                pad(o, depth + 1);
                quote_into(k, o);
                o.push_str(": ");
                pretty_into(x, depth + 1, o);
            }
            o.push('\n');
            pad(o, depth);
            o.push('}');
        }
    }
}

/// `jq -r` output of one value.
pub fn raw(v: &J) -> String {
    match v {
        J::Str(s) => format!("{s}\n"),
        v => format!("{}\n", pretty(v)),
    }
}

/// decNumber's to-scientific-string of a JSON number literal.
fn canonical(lit: &str) -> String {
    let (neg, s) = match lit.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, lit),
    };
    let (mant, exp) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], s[i + 1..].parse::<i64>().unwrap_or(0)),
        None => (s, 0),
    };
    let (int, frac) = mant.split_once('.').unwrap_or((mant, ""));
    let all = format!("{int}{frac}");
    let digits = all.trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let exp = exp - frac.len() as i64;
    let n = digits.len() as i64;
    let adjusted = exp + n - 1;
    let mut o = String::from(if neg { "-" } else { "" });
    if exp <= 0 && adjusted >= -6 {
        if exp == 0 {
            o.push_str(digits);
        } else if n > -exp {
            let at = (n + exp) as usize;
            o.push_str(&digits[..at]);
            o.push('.');
            o.push_str(&digits[at..]);
        } else {
            o.push_str("0.");
            o.extend(std::iter::repeat_n('0', (-exp - n) as usize));
            o.push_str(digits);
        }
    } else {
        o.push_str(&digits[..1]);
        if n > 1 {
            o.push('.');
            o.push_str(&digits[1..]);
        }
        o.push('E');
        if adjusted >= 0 {
            o.push('+');
        }
        o.push_str(&adjusted.to_string());
    }
    o
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn lit(&mut self, w: &str, v: J) -> Result<J, String> {
        if self.s[self.i..].starts_with(w.as_bytes()) {
            self.i += w.len();
            Ok(v)
        } else {
            Err("Invalid literal".into())
        }
    }

    fn value(&mut self, depth: usize) -> Result<J, String> {
        self.ws();
        if depth > 10000 {
            return Err("Exceeds depth limit for parsing".into());
        }
        let Some(&c) = self.s.get(self.i) else {
            return Err("Unfinished JSON term".into());
        };
        match c {
            b'n' => self.lit("null", J::Null),
            b't' => self.lit("true", J::Bool(true)),
            b'f' => self.lit("false", J::Bool(false)),
            b'"' => self.string().map(J::Str),
            b'[' => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(J::Arr(a));
                }
                loop {
                    a.push(self.value(depth + 1)?);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(J::Arr(a));
                        }
                        _ => return Err("Expected separator between values".into()),
                    }
                }
            }
            b'{' => {
                self.i += 1;
                let mut m = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(J::Obj(m));
                }
                loop {
                    self.ws();
                    if self.s.get(self.i) != Some(&b'"') {
                        return Err("Object keys must be strings".into());
                    }
                    let k = self.string()?;
                    self.ws();
                    if self.s.get(self.i) != Some(&b':') {
                        return Err("Objects must consist of key:value pairs".into());
                    }
                    self.i += 1;
                    let v = self.value(depth + 1)?;
                    J::obj_set(&mut m, &k, v);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(J::Obj(m));
                        }
                        _ => return Err("Expected separator between values".into()),
                    }
                }
            }
            b'-' | b'0'..=b'9' => {
                let st = self.i;
                while self.i < self.s.len()
                    && matches!(
                        self.s[self.i],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.i += 1;
                }
                let t = std::str::from_utf8(&self.s[st..self.i]).unwrap_or("");
                if serde_json::from_str::<serde_json::Number>(t).is_err() {
                    return Err("Invalid numeric literal".into());
                }
                Ok(J::Num(canonical(t)))
            }
            _ => Err("Invalid literal".into()),
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let h = std::str::from_utf8(self.s.get(self.i..self.i + 4)?).ok()?;
        let v = u32::from_str_radix(h, 16).ok()?;
        self.i += 4;
        Some(v)
    }

    fn string(&mut self) -> Result<String, String> {
        self.i += 1;
        let mut o: Vec<u8> = Vec::new();
        loop {
            let Some(&c) = self.s.get(self.i) else {
                return Err("Unfinished string".into());
            };
            self.i += 1;
            match c {
                b'"' => return Ok(String::from_utf8_lossy(&o).into_owned()),
                b'\\' => {
                    let Some(&e) = self.s.get(self.i) else {
                        return Err("Unfinished string".into());
                    };
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4().ok_or("Invalid escape")?;
                            if (0xd800..0xdc00).contains(&hi)
                                && self.s[self.i..].starts_with(b"\\u")
                            {
                                self.i += 2;
                                let lo = self.hex4().ok_or("Invalid escape")?;
                                if (0xdc00..0xe000).contains(&lo) {
                                    char::from_u32(0x10000 + ((hi - 0xd800) << 10) + (lo - 0xdc00))
                                        .unwrap_or('\u{fffd}')
                                } else {
                                    o.extend_from_slice("\u{fffd}".as_bytes());
                                    char::from_u32(lo).unwrap_or('\u{fffd}')
                                }
                            } else {
                                char::from_u32(hi).unwrap_or('\u{fffd}')
                            }
                        }
                        _ => return Err("Invalid escape".into()),
                    };
                    let mut b = [0; 4];
                    o.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
                }
                c => o.push(c),
            }
        }
    }
}

/// Every top-level value of `input` (jq reads its input as a stream) with the stdin line jq
/// reports for it, then the parse error, if any.
pub fn parse_stream(input: &str) -> (Vec<(J, usize)>, Option<String>) {
    let s = input.as_bytes();
    let mut p = P { s, i: 0 };
    let mut out = Vec::new();
    loop {
        p.ws();
        if p.i >= s.len() {
            return (out, None);
        }
        match p.value(0) {
            Ok(v) => {
                let eol = s[p.i..]
                    .iter()
                    .position(|&b| b == b'\n')
                    .map_or(s.len(), |n| p.i + n + 1);
                let line = s[..eol].iter().filter(|&&b| b == b'\n').count();
                out.push((v, line));
            }
            Err(e) => {
                let line = s[..p.i.min(s.len())]
                    .iter()
                    .filter(|&&b| b == b'\n')
                    .count()
                    + 1;
                return (out, Some(format!("jq: parse error: {e} at line {line}")));
            }
        }
    }
}

/// `printf '%s\n' "$input" | jq -r '.candidates[0].content.parts[0].text'`: (stdout, stderr,
/// exit code).
pub fn gemini_text(input: &str) -> (String, String, i32) {
    let (vals, perr) = parse_stream(input);
    let (mut out, mut err, mut code) = (String::new(), String::new(), 0);
    for (v, line) in vals {
        let r = field(&v, "candidates")
            .and_then(|v| index(&v, 0))
            .and_then(|v| field(&v, "content"))
            .and_then(|v| field(&v, "parts"))
            .and_then(|v| index(&v, 0))
            .and_then(|v| field(&v, "text"));
        match r {
            Ok(v) => out += &raw(&v),
            Err(e) => {
                err += &format!("jq: error (at <stdin>:{line}): {e}\n");
                code = 5;
            }
        }
    }
    if let Some(e) = perr {
        err += &e;
        err.push('\n');
        code = 2;
    }
    (out, err, code)
}
