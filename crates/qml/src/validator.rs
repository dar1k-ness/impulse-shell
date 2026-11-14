//! QValidator states for TextInput.validator (qvalidator.cpp, QLocaleData::validateChars) in the
//! en_US number format: `.` decimal point, `,` group separator.

use crate::runtime::World;
use crate::value::Value;

pub const INVALID: u8 = 0;
pub const INTERMEDIATE: u8 = 1;
pub const ACCEPTABLE: u8 = 2;

/// Hidden key of the Basic style SpinBox `validator: IntValidator { bottom/top: from..to }`.
pub const SPIN: &str = "\x01spinValidator";

enum Mode {
    Int,
    Standard,
    Scientific,
}

enum V {
    Int(i64, i64),
    Double(f64, f64, i64, Mode),
    Re(String, String),
}

fn num(w: &mut World, i: usize, n: &str, d: f64) -> f64 {
    match w.read(i, n) {
        Some(v) if !matches!(v, Value::Undefined | Value::Null) => v.to_number(),
        _ => d,
    }
}

fn spec(w: &mut World, v: &Value) -> Option<V> {
    match v {
        Value::Item(i) => {
            let i = *i;
            let types = w.items.get(i)?.types.clone();
            let has = |t: &str| types.iter().any(|x| x.as_str() == t);
            if has("IntValidator") {
                let b = num(w, i, "bottom", -2147483648.0) as i64;
                let t = num(w, i, "top", 2147483647.0) as i64;
                Some(V::Int(b, t))
            } else if has("DoubleValidator") {
                let b = num(w, i, "bottom", f64::NEG_INFINITY);
                let t = num(w, i, "top", f64::INFINITY);
                let d = num(w, i, "decimals", -1.0) as i64;
                let m = if num(w, i, "notation", 1.0) as i64 == 0 {
                    Mode::Standard
                } else {
                    Mode::Scientific
                };
                Some(V::Double(b, t, d, m))
            } else if has("RegularExpressionValidator") {
                Some(match w.read(i, "regularExpression") {
                    Some(Value::Obj(o)) => match &o.borrow().internal {
                        crate::value::Internal::RegExp { source, flags } => {
                            (source.to_string(), flags.to_string())
                        }
                        _ => (String::new(), String::new()),
                    },
                    Some(Value::Str(s)) => (s.to_string(), String::new()),
                    _ => (String::new(), String::new()),
                })
                .map(|(s, f)| V::Re(s, f))
            } else {
                None
            }
        }
        Value::Obj(o) => {
            let sb = match o.borrow().props.get(SPIN) {
                Some(Value::Item(s)) => *s,
                _ => return None,
            };
            let from = num(w, sb, "from", 0.0) as i64;
            let to = num(w, sb, "to", 99.0) as i64;
            Some(V::Int(from.min(to), from.max(to)))
        }
        _ => None,
    }
}

/// `field.validator`'s state for `text`, `None` without a (known) validator.
pub fn state(w: &mut World, field: usize, text: &str) -> Option<u8> {
    let v = w.validator_of(field)?;
    Some(match spec(w, &v)? {
        V::Int(b, t) => int(text, b, t),
        V::Double(b, t, d, m) => double(text, b, t, d, m),
        V::Re(s, f) => regex(text, &s, &f),
    })
}

/// QQuickTextInputPrivate::fixup: the validator's fixup of `text` when that makes it Acceptable.
pub fn fixup(w: &mut World, field: usize, text: &str) -> Option<String> {
    let v = w.validator_of(field)?;
    match spec(w, &v)? {
        // QIntValidator::fixup: locale().toString(parsed value).
        V::Int(b, t) => {
            let (buff, _) = validate_chars(text, &Mode::Int, -1).ok()?;
            let n: i64 = buff.parse().ok()?;
            let s = group(n);
            (int(&s, b, t) == ACCEPTABLE).then_some(s)
        }
        _ => None,
    }
}

fn group(n: i64) -> String {
    let d = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (k, c) in d.chars().enumerate() {
        if k > 0 && (d.len() - k).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// QLocaleData::validateChars: Err = Invalid, Ok((C-locale buffer, intermediate)).
fn validate_chars(s: &str, mode: &Mode, mut dec: i64) -> Result<(String, bool), ()> {
    #[derive(PartialEq)]
    enum St {
        Whole,
        Frac,
        Exp,
    }
    let sci = matches!(mode, Mode::Scientific);
    let mut st = St::Whole;
    let mut buff = String::new();
    let mut last = '\0';
    for ch in s.chars() {
        let c = match ch {
            '0'..='9' | '.' | ',' | '+' | '-' => ch,
            'e' | 'E' => 'e',
            '\u{2212}' => '-',
            _ => return Err(()),
        };
        if c.is_ascii_digit() {
            if st == St::Frac {
                if dec == 0 {
                    return Err(());
                }
                dec -= 1;
            }
        } else {
            match c {
                '.' if matches!(mode, Mode::Int) || st != St::Whole => return Err(()),
                '.' => st = St::Frac,
                '+' | '-' if last != '\0' && !(sci && last == 'e') => return Err(()),
                ',' if st != St::Whole || !last.is_ascii_digit() => return Err(()),
                'e' if !sci || st == St::Exp => return Err(()),
                'e' => st = St::Exp,
                _ => {}
            }
        }
        last = c;
        if c != ',' {
            buff.push(c);
        }
    }
    Ok((buff, matches!(last, ',' | '-' | '+' | 'e')))
}

/// QLocaleData::numberToCLocale grouping: the first group 1..3 digits, the others exactly 3.
fn grouping_ok(s: &str) -> bool {
    let whole = s.split(['.', 'e', 'E']).next().unwrap_or("");
    let whole = whole.trim_start_matches(['+', '-', '\u{2212}']);
    if !whole.contains(',') {
        return true;
    }
    let mut parts = whole.split(',');
    let first = parts.next().unwrap_or("").len();
    (1..=3).contains(&first) && parts.all(|p| p.len() == 3)
}

fn initial_check(b_neg: bool, t_neg: bool, r: Result<(String, bool), ()>) -> Result<String, u8> {
    let (buff, inter) = r.map_err(|_| INVALID)?;
    let Some(ch) = buff.chars().next() else {
        return Err(INTERMEDIATE);
    };
    if (!b_neg && ch == '-') || (t_neg && ch == '+') {
        return Err(INVALID);
    }
    if inter {
        return Err(INTERMEDIATE);
    }
    Ok(buff)
}

fn int(s: &str, b: i64, t: i64) -> u8 {
    let buff = match initial_check(b < 0, t < 0, validate_chars(s, &Mode::Int, -1)) {
        Ok(x) => x,
        Err(st) => return st,
    };
    let Ok(e) = buff.parse::<i64>() else {
        return INVALID;
    };
    if e >= b && e <= t {
        let ok = grouping_ok(s) && i32::try_from(e).is_ok();
        return if ok { ACCEPTABLE } else { INTERMEDIATE };
    }
    if e >= 0 {
        let len = buff.len() - usize::from(buff.starts_with('+'));
        let tl = if t != 0 {
            (t.unsigned_abs() as f64).log10() as usize + 1
        } else {
            1
        };
        if e > t && -e < b && len > tl {
            INVALID
        } else {
            INTERMEDIATE
        }
    } else if e < b {
        INVALID
    } else {
        INTERMEDIATE
    }
}

fn double(s: &str, b: f64, t: f64, dec: i64, m: Mode) -> u8 {
    let buff = match initial_check(b < 0.0, t < 0.0, validate_chars(s, &m, dec)) {
        Ok(x) => x,
        Err(st) => return st,
    };
    // QLocale::toDouble always parses scientific notation, and fails on a number too large
    // or too small for a double.
    let underflow = |i: f64| {
        let mantissa = buff.split('e').next().unwrap_or("");
        i == 0.0 && mantissa.chars().any(|c| matches!(c, '1'..='9'))
    };
    let i = match buff.parse::<f64>() {
        Ok(i) if grouping_ok(s) && i.is_finite() && !underflow(i) => i,
        _ => return INTERMEDIATE,
    };
    if i >= b && i <= t {
        return ACCEPTABLE;
    }
    if matches!(m, Mode::Standard) {
        let max = b.abs().max(t.abs()).floor();
        if max.is_finite() && max.abs() < 9.2e18 {
            let v = max as i64;
            let digits = if v == 0 {
                1
            } else {
                (v as f64).log10() as i32 + 1
            };
            let n = 10f64.powi(digits);
            if i.abs() > n - 10f64.powf(-(dec as f64)) {
                return INVALID;
            }
        }
    }
    INTERMEDIATE
}

/// PCRE2 (QRegularExpression) shorthand classes are ASCII; Rust's are Unicode.
fn ascii_classes(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let (mut class, mut it) = (false, src.chars());
    while let Some(c) = it.next() {
        match c {
            '\\' => match it.next() {
                Some(e @ ('d' | 'w' | 'D' | 'W')) => {
                    let set = if matches!(e, 'd' | 'D') {
                        "0-9"
                    } else {
                        "0-9A-Za-z_"
                    };
                    let neg = e.is_ascii_uppercase();
                    out.push_str(&match (class, neg) {
                        (true, false) => set.to_string(),
                        (false, false) => format!("[{set}]"),
                        (_, true) => format!("[^{set}]"),
                    });
                }
                Some('/') => out.push('/'),
                Some(e) => {
                    out.push('\\');
                    out.push(e);
                }
                None => out.push('\\'),
            },
            '[' if !class => {
                class = true;
                out.push(c);
                // `[]...]` / `[^]...]` keep the leading `]` literal.
            }
            ']' if class => {
                class = false;
                out.push(c);
            }
            '[' if class => out.push_str("\\["),
            _ => out.push(c),
        }
    }
    out
}

/// QRegularExpressionValidator::validate: anchored match, PartialPreferCompleteMatch.
fn regex(s: &str, src: &str, flags: &str) -> u8 {
    use regex_automata::hybrid::dfa::DFA;
    use regex_automata::util::{start, syntax};
    if src.is_empty() {
        return ACCEPTABLE;
    }
    let pat = format!("(?:{})\\z", ascii_classes(src));
    let syn = syntax::Config::new()
        .case_insensitive(flags.contains('i'))
        .multi_line(flags.contains('m'))
        .dot_matches_new_line(flags.contains('s'));
    let dfa = match DFA::builder().syntax(syn).build(&pat) {
        Ok(d) => d,
        Err(_) => return regex_fallback(s, src, flags),
    };
    let mut cache = dfa.create_cache();
    let cfg = start::Config::new().anchored(regex_automata::Anchored::Yes);
    let Ok(mut sid) = dfa.start_state(&mut cache, &cfg) else {
        return regex_fallback(s, src, flags);
    };
    for &byte in s.as_bytes() {
        match dfa.next_state(&mut cache, sid, byte) {
            Ok(n) => sid = n,
            Err(_) => return regex_fallback(s, src, flags),
        }
        if sid.is_dead() {
            return if s.is_empty() { INTERMEDIATE } else { INVALID };
        }
        if sid.is_quit() {
            return regex_fallback(s, src, flags);
        }
    }
    match dfa.next_eoi_state(&mut cache, sid) {
        Ok(e) if e.is_match() => ACCEPTABLE,
        Ok(_) => INTERMEDIATE,
        Err(_) => regex_fallback(s, src, flags),
    }
}

/// Patterns the DFA cannot run (look-around, back-references): complete matches only.
fn regex_fallback(s: &str, src: &str, flags: &str) -> u8 {
    let mut pre = String::new();
    for f in ['i', 'm', 's'] {
        if flags.contains(f) {
            pre.push(f);
        }
    }
    let pre = if pre.is_empty() {
        String::new()
    } else {
        format!("(?{pre})")
    };
    let re = fancy_regex::Regex::new(&format!("{pre}\\A(?:{})\\z", ascii_classes(src)));
    match re.ok().and_then(|r| r.is_match(s).ok()) {
        Some(true) => ACCEPTABLE,
        _ if s.is_empty() => INTERMEDIATE,
        _ => INVALID,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_validator_states_match_qintvalidator() {
        let st = |s| int(s, 0, 100);
        assert_eq!(st(""), INTERMEDIATE);
        assert_eq!(st("42"), ACCEPTABLE);
        assert_eq!(st("100"), ACCEPTABLE);
        assert_eq!(st("500"), INTERMEDIATE);
        assert_eq!(st("1000"), INVALID);
        assert_eq!(st("-1"), INVALID);
        assert_eq!(st("+"), INTERMEDIATE);
        assert_eq!(st("4a"), INVALID);
        assert_eq!(st("1.5"), INVALID);
        assert_eq!(int("1,000", 0, 5000), ACCEPTABLE);
        assert_eq!(int("1,00", 0, 5000), INTERMEDIATE);
        assert_eq!(int("41", 46, 53), INTERMEDIATE);
        assert_eq!(int("5", -100, -32), INTERMEDIATE);
        assert_eq!(int("-", -100, -32), INTERMEDIATE);
    }

    #[test]
    fn double_validator_states_match_qdoublevalidator() {
        let st = |s| double(s, 0.0, 10.0, 2, Mode::Standard);
        assert_eq!(st("3.14"), ACCEPTABLE);
        assert_eq!(st("3.141"), INVALID);
        assert_eq!(st("3."), ACCEPTABLE);
        assert_eq!(st("."), INTERMEDIATE);
        assert_eq!(st("50"), INTERMEDIATE);
        assert_eq!(st("100"), INVALID);
        assert_eq!(st("1e2"), INVALID);
        assert_eq!(double("1e2", 0.0, 1000.0, 2, Mode::Scientific), ACCEPTABLE);
        assert_eq!(double("1e", 0.0, 1000.0, 2, Mode::Scientific), INTERMEDIATE);
    }

    #[test]
    fn regex_validator_allows_prefixes() {
        assert_eq!(regex("ab", "[a-z]{2}\\d{2}", ""), INTERMEDIATE);
        assert_eq!(regex("ab12", "[a-z]{2}\\d{2}", ""), ACCEPTABLE);
        assert_eq!(regex("ab123", "[a-z]{2}\\d{2}", ""), INVALID);
        assert_eq!(regex("1", "[a-z]{2}\\d{2}", ""), INVALID);
        assert_eq!(regex("", "[a-z]+", ""), INTERMEDIATE);
        assert_eq!(regex("AB", "[a-z]+", "i"), ACCEPTABLE);
        assert_eq!(regex("٣", "\\d", ""), INVALID);
    }
}
