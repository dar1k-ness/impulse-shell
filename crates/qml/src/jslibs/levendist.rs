//! The singleton `Levenshtein`: string similarity scores, written from the original's former
//! `functions/levendist.js`, on UTF-16 code units like that script.

use crate::value::Value;

fn units(v: Option<&Value>) -> Vec<u16> {
    match v {
        Some(Value::Str(s)) => s.encode_utf16().collect(),
        Some(v) => v.to_js_string().encode_utf16().collect(),
        None => "undefined".encode_utf16().collect(),
    }
}

fn includes(h: &[u16], n: &[u16]) -> bool {
    n.is_empty() || h.windows(n.len()).any(|w| w == n)
}

pub fn levenshtein(s1: &[u16], s2: &[u16]) -> usize {
    strsim::generic_levenshtein(&Units(s1), &Units(s2))
}

/// JS strings compare by UTF-16 code unit.
struct Units<'a>(&'a [u16]);

impl<'a> IntoIterator for &Units<'a> {
    type Item = u16;
    type IntoIter = std::iter::Copied<std::slice::Iter<'a, u16>>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter().copied()
    }
}

pub fn partial_ratio(short: &[u16], long: &[u16]) -> f64 {
    if short.is_empty() {
        return 1.0;
    }
    let mut best = 0.0;
    if long.len() >= short.len() {
        for i in 0..=long.len() - short.len() {
            let dist = levenshtein(short, &long[i..i + short.len()]);
            let score = 1.0 - (dist as f64 / short.len() as f64);
            if score > best {
                best = score;
            }
        }
    }
    best
}

fn common_prefix(s1: &[u16], s2: &[u16]) -> f64 {
    s1.iter().zip(s2).take_while(|(a, b)| a == b).count() as f64
}

/// The shared head of `computeScore` / `computeTextMatchScore`: `None` for an early `1.0`.
fn parts(s1: &[u16], s2: &[u16]) -> Option<(f64, f64, f64)> {
    if s1 == s2 {
        return None;
    }
    let dist = levenshtein(s1, s2) as f64;
    let max_len = s1.len().max(s2.len()) as f64;
    if max_len == 0.0 {
        return None;
    }
    let full = 1.0 - (dist / max_len);
    let part = if s1.len() < s2.len() {
        partial_ratio(s1, s2)
    } else {
        partial_ratio(s2, s1)
    };
    Some((full, part, max_len))
}

fn clamp01(score: f64) -> f64 {
    0f64.max(1f64.min(score))
}

pub fn compute_score(s1: &[u16], s2: &[u16]) -> f64 {
    let Some((full, part, max_len)) = parts(s1, s2) else {
        return 1.0;
    };
    let mut score = 0.85 * full + 0.15 * part;
    if !s1.is_empty() && !s2.is_empty() && s1[0] != s2[0] {
        score -= 0.05;
    }
    let len_diff = s1.len().abs_diff(s2.len()) as f64;
    if len_diff >= 3.0 {
        score -= 0.05 * len_diff / max_len;
    }
    score += 0.02 * common_prefix(s1, s2);
    if includes(s1, s2) || includes(s2, s1) {
        score += 0.06;
    }
    clamp01(score)
}

pub fn compute_text_match_score(s1: &[u16], s2: &[u16]) -> f64 {
    let Some((full, part, max_len)) = parts(s1, s2) else {
        return 1.0;
    };
    let mut score = 0.4 * full + 0.6 * part;
    let len_diff = s1.len().abs_diff(s2.len()) as f64;
    if len_diff >= 10.0 {
        score -= 0.02 * len_diff / max_len;
    }
    score += 0.01 * common_prefix(s1, s2);
    if includes(s1, s2) || includes(s2, s1) {
        score += 0.2;
    }
    clamp01(score)
}

fn two(a: &[Value], f: fn(&[u16], &[u16]) -> f64) -> Value {
    Value::Num(f(&units(a.first()), &units(a.get(1))))
}

/// The `Levenshtein` singleton of the `Impulse` module.
pub static TYPE: crate::impulse::Type = crate::impulse::Type {
    name: "Levenshtein",
    methods: &[
        ("computeScore", |_, _, a| Ok(two(a, compute_score))),
        ("computeTextMatchScore", |_, _, a| {
            Ok(two(a, compute_text_match_score))
        }),
    ],
    init: None,
};
