//! The singleton `FuzzySort`: farzher/fuzzysort 3, written statement for statement from the
//! script the original shipped (kept as `testdata/fuzzysort.js` for the test below), on UTF-16 code units. Results keep the JS sharing: a result's `_indexes` is its prepared
//! target's, and `_score` / `_nextBeginningIndexes` live on the prepared target.
//! `options.keys` / `options.scoreFn` (unused by the original) are not ported.
//!
//! The script keeps every distinct search and string target in caches that only its
//! `cleanup()` empties; the original never called it and it is not ported. Here the caches are bounded
//! without changing any result: prepared searches are
//! immutable and internal (dropping one only costs a re-prepare), and a prepared target is
//! dropped only while nothing outside the cache can reach it or its shared state.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::builtins::qstring_lower;
use crate::eval::{Callable, Interp, NativeFn, R, define_accessor, fn_value, new_obj, set_own};
use crate::value::{Obj, Value};

#[derive(Default)]
struct Indexes {
    v: Vec<i64>,
    len: usize,
}

struct Prep {
    /// `_targetLower` as code units.
    lower: Vec<u16>,
    /// `_targetLowerCodes` (`NaN` where `toLowerCase` shortened the string).
    codes: Vec<f64>,
    bitflags: i32,
    next_beginning: RefCell<Option<Vec<i64>>>,
}

/// A JS `Result`; `prep` is set on prepared targets (`isPrepared`).
struct Res {
    target: Value,
    score: Cell<f64>,
    indexes: Rc<RefCell<Indexes>>,
    prep: Option<Rc<Prep>>,
}

struct Search {
    codes: Vec<f64>,
    lower: Vec<u16>,
    contains_space: bool,
    bitflags: i32,
    spaces: Vec<Search>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Str(Rc<str>),
    Num(String),
}

/// Prepared-target count that triggers a sweep of unreachable entries (the threshold then
/// follows twice the survivors, so sweeps stay amortized O(1) per insert).
const PREPARED_SWEEP: usize = 1024;
/// Prepared searches kept before the cache starts over.
const SEARCHES_MAX: usize = 1024;

thread_local! {
    static PROTO: OnceCell<Obj> = const { OnceCell::new() };
    static PREPARED: RefCell<HashMap<Key, Value>> = RefCell::new(HashMap::new());
    static SEARCHES: RefCell<HashMap<Key, Rc<Search>>> = RefCell::new(HashMap::new());
    static SWEEP_AT: Cell<usize> = const { Cell::new(PREPARED_SWEEP) };
    static LATIN: fancy_regex::Regex = fancy_regex::Regex::new(r"\p{Script=Latin}+").unwrap();
    static SPACES: fancy_regex::Regex = fancy_regex::Regex::new(r"\s+").unwrap();
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `remove_accents`: NFD on Latin runs, then drop U+0300..U+036F.
fn remove_accents(s: &str) -> String {
    let nfd = icu_normalizer::DecomposingNormalizerBorrowed::new_nfd();
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    LATIN.with(|re| {
        for m in re.find_iter(s).flatten() {
            out.push_str(&s[last..m.start()]);
            out.push_str(&nfd.normalize(m.as_str()));
            last = m.end();
        }
    });
    out.push_str(&s[last..]);
    out.retain(|c| !('\u{300}'..='\u{36f}').contains(&c));
    out
}

/// `prepareLowerInfo`: (lowerCodes, bitflags, containsSpace, _lower).
fn lower_info(s: &str) -> (Vec<f64>, i32, bool, Vec<u16>) {
    let s = remove_accents(s);
    let lower: Vec<u16> = qstring_lower(&s).encode_utf16().collect();
    let mut codes = Vec::new();
    let mut bitflags = 0i32;
    let mut contains_space = false;
    for i in 0..utf16_len(&s) {
        let c = lower.get(i).map_or(f64::NAN, |&c| c as f64);
        codes.push(c);
        if c == 32.0 {
            contains_space = true;
            continue;
        }
        let bit = if (97.0..=122.0).contains(&c) {
            c as i32 - 97
        } else if (48.0..=57.0).contains(&c) {
            26
        } else if c <= 127.0 {
            30
        } else {
            31
        };
        bitflags |= 1 << bit;
    }
    (codes, bitflags, contains_space, lower)
}

fn next_beginning_indexes(target: &str) -> Vec<i64> {
    let t: Vec<u16> = remove_accents(target).encode_utf16().collect();
    let mut beginning = Vec::new();
    let (mut was_upper, mut was_alnum) = (false, false);
    for (i, &c) in t.iter().enumerate() {
        let upper = (65..=90).contains(&c);
        let alnum = upper || (97..=122).contains(&c) || (48..=57).contains(&c);
        if upper && !was_upper || !was_alnum || !alnum {
            beginning.push(i as i64);
        }
        was_upper = upper;
        was_alnum = alnum;
    }
    let len = t.len() as i64;
    let mut out = Vec::with_capacity(t.len());
    let mut last = beginning.first().copied();
    let mut last_i = 0;
    for i in 0..len {
        if last.is_some_and(|l| l > i) {
            out.push(last.unwrap());
        } else {
            last_i += 1;
            last = beginning.get(last_i).copied();
            out.push(last.unwrap_or(len));
        }
    }
    out
}

fn str_of(v: &Value) -> Rc<str> {
    match v {
        Value::Str(s) => s.clone(),
        Value::Num(_) => v.to_js_string().into(),
        _ => "".into(),
    }
}

fn prepare_res(target: &Value) -> Res {
    let t = str_of(target);
    let (codes, bitflags, _, lower) = lower_info(&t);
    Res {
        target: Value::Str(t),
        score: Cell::new(f64::NEG_INFINITY),
        indexes: Default::default(),
        prep: Some(Rc::new(Prep {
            lower,
            codes,
            bitflags,
            next_beginning: RefCell::new(None),
        })),
    }
}

fn proto() -> Obj {
    PROTO.with(|p| {
        p.get_or_init(|| {
            let o = new_obj();
            let getter = |name: &'static str, f: NativeFn| {
                define_accessor(&o, name, true, fn_value(Callable::Native(name, f)), false);
            };
            getter("score", |_, this, _| {
                Ok(Value::Num(
                    res(this).map_or(0.0, |r| normalize_score(r.score.get())),
                ))
            });
            getter("_score", |_, this, _| {
                Ok(Value::Num(
                    res(this).map_or(f64::NEG_INFINITY, |r| r.score.get()),
                ))
            });
            getter("indexes", |_, this, _| {
                let Some(r) = res(this) else {
                    return Ok(Value::array(Vec::new()));
                };
                let ix = r.indexes.borrow();
                let mut v: Vec<i64> = ix.v.iter().take(ix.len).copied().collect();
                v.sort_unstable();
                Ok(Value::array(
                    v.into_iter().map(|x| Value::Num(x as f64)).collect(),
                ))
            });
            set_own(
                &o,
                "highlight",
                fn_value(Callable::Native("highlight", |it, this, a| {
                    highlight(it, this, a.first(), a.get(1))
                })),
            );
            o
        })
        .clone()
    })
}

fn res(v: &Value) -> Option<Rc<Res>> {
    match v {
        Value::Obj(o) => o.borrow().native.clone()?.downcast::<Res>().ok(),
        _ => None,
    }
}

fn prepared(v: &Value) -> Option<(Rc<Res>, Rc<Prep>)> {
    let r = res(v)?;
    let p = r.prep.clone()?;
    Some((r, p))
}

/// The JS object for `r`; `obj` is set as an own property when given.
fn res_value(r: Rc<Res>, obj: Option<Value>) -> Value {
    let o = new_obj();
    set_own(&o, "target", r.target.clone());
    if let Some(v) = obj {
        set_own(&o, "obj", v);
    }
    {
        let mut b = o.borrow_mut();
        b.proto = Some(proto());
        b.native = Some(r);
    }
    Value::Obj(o)
}

fn pow(x: f64, y: f64) -> f64 {
    x.powf(std::hint::black_box(y))
}

fn normalize_score(score: f64) -> f64 {
    if score == f64::NEG_INFINITY {
        return 0.0;
    }
    if score > 1.0 {
        return score;
    }
    pow(
        std::f64::consts::E,
        (pow(-score + 1.0, 0.04307) - 1.0) * -2.0,
    )
}

fn denormalize_score(n: f64) -> f64 {
    if n == 0.0 {
        return f64::NEG_INFINITY;
    }
    if n > 1.0 {
        return n;
    }
    1.0 - pow(n.ln() / -2.0 + 1.0, 1.0 / 0.04307)
}

fn cache_key(v: &Value) -> Option<Key> {
    match v {
        Value::Str(s) => Some(Key::Str(s.clone())),
        Value::Num(_) => Some(Key::Num(v.to_js_string())),
        _ => None,
    }
}

fn prepare_search(search: &Value) -> Search {
    let s = str_of(search);
    let s = s.trim();
    let (codes, bitflags, contains_space, lower) = lower_info(s);
    let mut spaces = Vec::new();
    if contains_space {
        let mut parts: Vec<&str> = Vec::new();
        let mut last = 0;
        SPACES.with(|re| {
            for m in re.find_iter(s).flatten() {
                parts.push(&s[last..m.start()]);
                last = m.end();
            }
        });
        parts.push(&s[last..]);
        let mut seen: Vec<&str> = Vec::new();
        for p in parts {
            if !seen.contains(&p) {
                seen.push(p);
            }
        }
        for p in seen {
            if p.is_empty() {
                continue;
            }
            let (codes, bitflags, _, _) = lower_info(p);
            let lower = qstring_lower(p).encode_utf16().collect();
            spaces.push(Search {
                codes,
                lower,
                contains_space: false,
                bitflags,
                spaces: Vec::new(),
            });
        }
    }
    Search {
        codes,
        lower,
        contains_space,
        bitflags,
        spaces,
    }
}

fn get_prepared(target: &Value) -> Value {
    let key = cache_key(target);
    let huge = matches!(target, Value::Str(s) if utf16_len(s) > 999);
    let Some(key) = key.filter(|_| !huge) else {
        return res_value(Rc::new(prepare_res(target)), Some(Value::Null));
    };
    if let Some(v) = PREPARED.with(|c| c.borrow().get(&key).cloned()) {
        return v;
    }
    let v = res_value(Rc::new(prepare_res(target)), Some(Value::Null));
    PREPARED.with(|c| {
        let mut c = c.borrow_mut();
        let at = SWEEP_AT.get();
        if c.len() >= at {
            c.retain(|_, v| !unreachable(v));
            SWEEP_AT.set((c.len() * 2).max(PREPARED_SWEEP));
        }
        c.insert(key, v.clone());
    });
    v
}

/// Whether the cached prepared target `v` is reachable only through the cache and still as
/// `getPrepared` made it (no own property added or changed). Re-preparing it then gives an
/// equal target: every match and `all` rewrite `_score` / `_indexes` before reading them,
/// and `_nextBeginningIndexes` is recomputed identically.
fn unreachable(v: &Value) -> bool {
    let Value::Obj(o) = v else { return true };
    if Rc::strong_count(o) != 1 {
        return false;
    }
    let b = o.borrow();
    let Some(n) = &b.native else { return false };
    let Some(r) = n.downcast_ref::<Res>() else {
        return false;
    };
    let pristine = b.keys.len() == 2
        && b.props.len() == 2
        && b.array.is_none()
        && b.call.is_none()
        && matches!(b.props.get("obj"), Some(Value::Null))
        && matches!((b.props.get("target"), &r.target), (Some(Value::Str(a)), Value::Str(t)) if Rc::ptr_eq(a, t))
        && b.proto.as_ref().is_some_and(|p| Rc::ptr_eq(p, &proto()));
    pristine
        && Rc::strong_count(n) == 1
        && Rc::strong_count(&r.indexes) == 1
        && r.prep.as_ref().is_some_and(|p| Rc::strong_count(p) == 1)
}

fn get_prepared_search(search: &Value) -> Rc<Search> {
    let key = cache_key(search);
    let huge = matches!(search, Value::Str(s) if utf16_len(s) > 999);
    let Some(key) = key.filter(|_| !huge) else {
        return Rc::new(prepare_search(search));
    };
    if let Some(s) = SEARCHES.with(|c| c.borrow().get(&key).cloned()) {
        return s;
    }
    let s = Rc::new(prepare_search(search));
    SEARCHES.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= SEARCHES_MAX {
            c.clear();
        }
        c.insert(key, s.clone());
    });
    s
}

/// `isPrepared(x) ? x : getPrepared(x)`, as the prepared value plus its native state.
fn ensure_prepared(v: &Value) -> (Value, Rc<Res>, Rc<Prep>) {
    if let Some((r, p)) = prepared(v) {
        return (v.clone(), r, p);
    }
    let v = get_prepared(v);
    let (r, p) = prepared(&v).expect("prepared");
    (v, r, p)
}

/// JS `a === b` on array reads, where out-of-range reads are `undefined`.
fn same(a: Option<&f64>, b: Option<&f64>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => x == y,
        (None, None) => true,
        _ => false,
    }
}

fn index_of(h: &[u16], n: &[u16], from: i64) -> i64 {
    let from = from.clamp(0, h.len() as i64) as usize;
    if n.is_empty() {
        return from as i64;
    }
    if n.len() > h.len() {
        return -1;
    }
    (from..=h.len() - n.len())
        .find(|&i| h[i..i + n.len()] == *n)
        .map_or(-1, |i| i as i64)
}

fn algorithm(s: &Search, r: &Res, p: &Prep, allow_spaces: bool) -> Option<Rc<Res>> {
    if !allow_spaces && s.contains_space {
        return algorithm_spaces(s, r, p);
    }
    let sc = &s.codes;
    let tc = &p.codes;
    let slen = sc.len();
    let tlen = tc.len() as i64;
    let mut si = 0;
    let mut ti: i64 = 0;
    let mut simple: Vec<i64> = Vec::with_capacity(slen);
    let mut code = sc.first();
    loop {
        if same(code, tc.get(ti as usize)) {
            simple.push(ti);
            si += 1;
            if si == slen {
                break;
            }
            code = sc.get(si);
        }
        ti += 1;
        if ti >= tlen {
            return None;
        }
    }

    let mut si: i64 = 0;
    let mut success = false;
    let mut strict = vec![0i64; slen];
    let mut strict_len = 0usize;
    let mut nbi_slot = p.next_beginning.borrow_mut();
    if nbi_slot.is_none() {
        *nbi_slot = Some(next_beginning_indexes(&str_of(&r.target)));
    }
    let nbi: &Vec<i64> = nbi_slot.as_ref().unwrap();
    let next = |i: i64| {
        usize::try_from(i)
            .ok()
            .and_then(|i| nbi.get(i).copied())
            .unwrap_or(tlen)
    };
    ti = if simple[0] == 0 {
        0
    } else {
        next(simple[0] - 1)
    };
    let mut backtrack = 0;
    if ti != tlen {
        loop {
            if ti >= tlen {
                if si <= 0 {
                    break;
                }
                backtrack += 1;
                if backtrack > 200 {
                    break;
                }
                si -= 1;
                strict_len -= 1;
                ti = next(strict[strict_len]);
            } else if same(sc.get(si as usize), tc.get(ti as usize)) {
                strict[strict_len] = ti;
                strict_len += 1;
                si += 1;
                if si as usize == slen {
                    success = true;
                    break;
                }
                ti += 1;
            } else {
                ti = next(ti);
            }
        }
    }

    let mut sub = if slen <= 1 {
        -1
    } else {
        index_of(&p.lower, &s.lower, simple[0])
    };
    let is_sub = sub != -1;
    let mut is_sub_beginning = is_sub
        && (sub == 0 || usize::try_from(sub - 1).ok().and_then(|i| nbi.get(i)) == Some(&sub));
    if is_sub && !is_sub_beginning {
        let mut i = 0usize;
        while i < nbi.len() {
            if i as i64 <= sub {
                i = nbi[i] as usize;
                continue;
            }
            let mut k = 0;
            while k < slen {
                if !same(sc.get(k), tc.get(i + k)) {
                    break;
                }
                k += 1;
            }
            if k == slen {
                sub = i as i64;
                is_sub_beginning = true;
                break;
            }
            i = nbi[i] as usize;
        }
    }

    let calculate = |m: &[i64]| -> f64 {
        let mut score = 0.0;
        let mut extra = 0.0;
        for i in 1..slen {
            if m[i] - m[i - 1] != 1 {
                score -= m[i] as f64;
                extra += 1.0;
            }
        }
        let unmatched = (m[slen - 1] - m[0] - (slen as i64 - 1)) as f64;
        score -= (12.0 + unmatched) * extra;
        if m[0] != 0 {
            score -= m[0] as f64 * m[0] as f64 * 0.2;
        }
        if !success {
            score *= 1000.0;
        } else {
            let mut unique = 1.0;
            let mut i = nbi.first().copied();
            while let Some(x) = i.filter(|&x| x < tlen) {
                unique += 1.0;
                i = nbi.get(x as usize).copied();
            }
            if unique > 24.0 {
                score *= (unique - 24.0) * 10.0;
            }
        }
        let sl = slen as f64;
        score -= (tlen as f64 - sl) / 2.0;
        if is_sub {
            score /= 1.0 + sl * sl * 1.0;
        }
        if is_sub_beginning {
            score /= 1.0 + sl * sl * 1.0;
        }
        score -= (tlen as f64 - sl) / 2.0;
        score
    };

    let substring = |simple: &mut Vec<i64>| {
        for (i, x) in simple.iter_mut().enumerate() {
            *x = sub + i as i64;
        }
    };
    let (best, score) = if !success {
        if is_sub {
            substring(&mut simple);
        }
        let score = calculate(&simple);
        (simple, score)
    } else if is_sub_beginning {
        substring(&mut simple);
        let score = calculate(&simple);
        (simple, score)
    } else {
        let score = calculate(&strict);
        (strict, score)
    };
    drop(nbi_slot);

    r.score.set(score);
    {
        let mut ix = r.indexes.borrow_mut();
        for (i, &x) in best.iter().enumerate() {
            if i < ix.v.len() {
                ix.v[i] = x;
            } else {
                ix.v.push(x);
            }
        }
        ix.len = slen;
    }
    Some(Rc::new(Res {
        target: r.target.clone(),
        score: Cell::new(score),
        indexes: r.indexes.clone(),
        prep: None,
    }))
}

fn algorithm_spaces(s: &Search, target: &Res, p: &Prep) -> Option<Rc<Res>> {
    let mut seen: Vec<i64> = Vec::new();
    let mut score = 0.0;
    let mut result = None;
    let mut first_seen = 0i64;
    let n = s.spaces.len();
    let mut changes: Vec<(usize, i64)> = Vec::new();
    let reset = |changes: &[(usize, i64)]| {
        if let Some(nbi) = p.next_beginning.borrow_mut().as_mut() {
            for &(i, v) in changes.iter().rev() {
                nbi[i] = v;
            }
        }
    };
    for (i, search) in s.spaces.iter().enumerate() {
        let Some(r) = algorithm(search, target, p, false) else {
            reset(&changes);
            return None;
        };
        let ix = r.indexes.borrow();
        if i != n - 1 {
            let consecutive = (0..ix.len.saturating_sub(1)).all(|k| ix.v[k + 1] - ix.v[k] == 1);
            if consecutive {
                let nb = ix.v[ix.len - 1] + 1;
                let mut slot = p.next_beginning.borrow_mut();
                let nbi = slot.as_mut().unwrap();
                let to_replace = nbi[(nb - 1) as usize];
                for k in (0..nb as usize).rev() {
                    if to_replace != nbi[k] {
                        break;
                    }
                    nbi[k] = nb;
                    changes.push((k, to_replace));
                }
            }
        }
        score += r.score.get() / n as f64;
        if ix.v[0] < first_seen {
            score -= (first_seen - ix.v[0]) as f64 * 2.0;
        }
        first_seen = ix.v[0];
        for &x in &ix.v[..ix.len] {
            if !seen.contains(&x) {
                seen.push(x);
            }
        }
        drop(ix);
        result = Some(r);
    }
    reset(&changes);
    if let Some(a) = algorithm(s, target, p, true)
        && a.score.get() > score
    {
        return Some(a);
    }
    let result = result?;
    result.score.set(score);
    let mut ix = result.indexes.borrow_mut();
    for (i, &x) in seen.iter().enumerate() {
        if i < ix.v.len() {
            ix.v[i] = x;
        } else {
            ix.v.push(x);
        }
    }
    ix.len = seen.len();
    drop(ix);
    Some(result)
}

/// `getValue(obj, prop)`.
fn get_value(it: &mut Interp, obj: &Value, prop: &Value) -> R<Value> {
    let name = prop.to_js_string();
    let tmp = it.get(obj, &name)?;
    if !matches!(tmp, Value::Undefined) {
        return Ok(tmp);
    }
    if prop.type_of() == "function" {
        return it.call(prop, Value::Undefined, std::slice::from_ref(obj));
    }
    let segs: Vec<Value> = if let Value::Obj(o) = prop
        && let Some(a) = &o.borrow().array
    {
        a.clone()
    } else {
        name.split('.').map(Value::str).collect()
    };
    let mut obj = obj.clone();
    for seg in segs {
        if !obj.truthy() {
            break;
        }
        obj = it.get(&obj, &seg.to_js_string())?;
    }
    Ok(obj)
}

fn opt(it: &mut Interp, options: &Value, k: &str) -> R<Value> {
    if matches!(options, Value::Undefined | Value::Null) {
        return Ok(Value::Undefined);
    }
    it.get(options, k)
}

fn elems(it: &mut Interp, v: &Value) -> R<Vec<Value>> {
    crate::shapes::elems(it, v)
}

fn with_total(v: Vec<Value>, total: f64) -> Value {
    let a = Value::array(v);
    if let Value::Obj(o) = &a {
        set_own(o, "total", Value::Num(total));
    }
    a
}

fn limit_of(it: &mut Interp, options: &Value) -> R<f64> {
    let l = opt(it, options, "limit")?;
    Ok(if l.truthy() {
        l.to_number()
    } else {
        f64::INFINITY
    })
}

fn all(it: &mut Interp, targets: &Value, options: &Value) -> R<Value> {
    let total = it.get(targets, "length")?.to_number();
    let limit = limit_of(it, options)?;
    let key = opt(it, options, "key")?;
    if !key.truthy() && opt(it, options, "keys")?.truthy() {
        return it.throw(
            "TypeError",
            "fuzzysort: options.keys is not supported natively",
        );
    }
    let mut out = Vec::new();
    for obj in elems(it, targets)? {
        let entry = if key.truthy() {
            let t = get_value(it, &obj, &key)?;
            if matches!(t, Value::Undefined | Value::Null) {
                continue;
            }
            let (_, r, _) = ensure_prepared(&t);
            let obj = if matches!(obj, Value::Undefined) {
                Value::Null
            } else {
                obj
            };
            let fresh = Res {
                target: r.target.clone(),
                score: Cell::new(r.score.get()),
                indexes: Default::default(),
                prep: None,
            };
            res_value(Rc::new(fresh), Some(obj))
        } else {
            if matches!(obj, Value::Undefined | Value::Null) {
                continue;
            }
            let (v, r, _) = ensure_prepared(&obj);
            r.score.set(f64::NEG_INFINITY);
            r.indexes.borrow_mut().len = 0;
            v
        };
        out.push(entry);
        if out.len() as f64 >= limit {
            break;
        }
    }
    Ok(with_total(out, total))
}

/// The original's hacked FastPriorityQueue: a min-heap on `_score`.
struct Queue(Vec<(f64, Value)>);

#[allow(clippy::neg_cmp_op_on_partial_ord)] // `!(a < b)` keeps the JS NaN behavior
impl Queue {
    fn add(&mut self, r: (f64, Value)) {
        let mut a = self.0.len();
        self.0.push(r.clone());
        while a > 0 {
            let v = (a - 1) >> 1;
            if !(r.0 < self.0[v].0) {
                break;
            }
            self.0[a] = self.0[v].clone();
            a = v;
        }
        self.0[a] = r;
    }

    fn sift(&mut self) {
        let o = self.0.len();
        let mut a = 0;
        let v = self.0[0].clone();
        let mut c = 1;
        while c < o {
            let s = c + 1;
            a = c;
            if s < o && self.0[s].0 < self.0[c].0 {
                a = s;
            }
            self.0[(a - 1) >> 1] = self.0[a].clone();
            c = 1 + (a << 1);
        }
        while a > 0 {
            let f = (a - 1) >> 1;
            if !(v.0 < self.0[f].0) {
                break;
            }
            self.0[a] = self.0[f].clone();
            a = f;
        }
        self.0[a] = v;
    }

    fn poll(&mut self) -> Option<Value> {
        if self.0.is_empty() {
            return None;
        }
        let last = self.0.pop().unwrap();
        if self.0.is_empty() {
            return Some(last.1);
        }
        let top = std::mem::replace(&mut self.0[0], last);
        self.sift();
        Some(top.1)
    }

    fn replace_top(&mut self, r: (f64, Value)) {
        self.0[0] = r;
        self.sift();
    }
}

pub(crate) fn go(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let search = a.first().cloned().unwrap_or_default();
    let targets = a.get(1).cloned().unwrap_or_default();
    let options = a.get(2).cloned().unwrap_or_default();
    if !search.truthy() {
        return if opt(it, &options, "all")?.truthy() {
            all(it, &targets, &options)
        } else {
            Ok(with_total(Vec::new(), 0.0))
        };
    }
    let ps = get_prepared_search(&search);
    let th = opt(it, &options, "threshold")?;
    let threshold = denormalize_score(if th.truthy() { th.to_number() } else { 0.0 });
    let limit = limit_of(it, &options)?;
    let key = opt(it, &options, "key")?;
    if !key.truthy() && opt(it, &options, "keys")?.truthy() {
        return it.throw(
            "TypeError",
            "fuzzysort: options.keys is not supported natively",
        );
    }
    let mut q = Queue(Vec::new());
    let (mut len, mut limited) = (0usize, 0usize);
    for obj in elems(it, &targets)? {
        let target = if key.truthy() {
            get_value(it, &obj, &key)?
        } else {
            obj.clone()
        };
        if !target.truthy() {
            continue;
        }
        let (_, r, p) = ensure_prepared(&target);
        if ps.bitflags & p.bitflags != ps.bitflags {
            continue;
        }
        let Some(result) = algorithm(&ps, &r, &p, false) else {
            continue;
        };
        let score = result.score.get();
        if score < threshold {
            continue;
        }
        let v = res_value(result, key.truthy().then_some(obj));
        if (len as f64) < limit {
            q.add((score, v));
            len += 1;
        } else {
            limited += 1;
            if score > q.0[0].0 {
                q.replace_top((score, v));
            }
        }
    }
    if len == 0 {
        return Ok(with_total(Vec::new(), 0.0));
    }
    let mut out = vec![Value::Undefined; len];
    for slot in out.iter_mut().rev() {
        *slot = q.poll().unwrap_or_default();
    }
    Ok(with_total(out, (len + limited) as f64))
}

fn substr(t: &[u16], from: usize) -> String {
    String::from_utf16_lossy(t.get(from..).unwrap_or(&[]))
}

fn highlight(
    it: &mut Interp,
    result: &Value,
    open: Option<&Value>,
    close: Option<&Value>,
) -> R<Value> {
    let callback = open.filter(|v| v.type_of() == "function").cloned();
    let text = |v: Option<&Value>, d: &str| match v {
        None | Some(Value::Undefined) => d.to_string(),
        Some(v) => v.to_js_string(),
    };
    let (open_s, close_s) = (text(open, "<b>"), text(close, "</b>"));
    let target: Vec<u16> = it
        .get(result, "target")?
        .to_js_string()
        .encode_utf16()
        .collect();
    let indexes: Vec<f64> = {
        let v = it.get(result, "indexes")?;
        elems(it, &v)?.iter().map(Value::to_number).collect()
    };
    // UTF-16 units, as the string is in JS: a surrogate pair stays one character.
    let mut highlighted: Vec<u16> = Vec::new();
    let take = |h: &mut Vec<u16>| Value::str(&String::from_utf16_lossy(&std::mem::take(h)));
    let mut match_i = 0.0;
    let mut indexes_i = 0;
    let mut opened = false;
    let mut parts: Vec<Value> = Vec::new();
    for (i, &unit) in target.iter().enumerate() {
        if indexes.get(indexes_i) == Some(&(i as f64)) {
            indexes_i += 1;
            if !opened {
                opened = true;
                if callback.is_some() {
                    parts.push(take(&mut highlighted));
                } else {
                    highlighted.extend(open_s.encode_utf16());
                }
            }
            if indexes_i == indexes.len() {
                if let Some(cb) = &callback {
                    highlighted.push(unit);
                    let h = take(&mut highlighted);
                    parts.push(it.call(cb, Value::Undefined, &[h, Value::Num(match_i)])?);
                    parts.push(Value::str(&substr(&target, i + 1)));
                } else {
                    highlighted.push(unit);
                    highlighted.extend(close_s.encode_utf16());
                    highlighted.extend_from_slice(target.get(i + 1..).unwrap_or(&[]));
                }
                return Ok(match callback {
                    Some(_) => Value::array(parts),
                    None => take(&mut highlighted),
                });
            }
        } else if opened {
            opened = false;
            if let Some(cb) = &callback {
                let h = take(&mut highlighted);
                parts.push(it.call(cb, Value::Undefined, &[h, Value::Num(match_i)])?);
                match_i += 1.0;
            } else {
                highlighted.extend(close_s.encode_utf16());
            }
        }
        highlighted.push(unit);
    }
    Ok(match callback {
        Some(_) => Value::array(parts),
        None => take(&mut highlighted),
    })
}

/// `fuzzysort.prepare(target)`.
pub(crate) fn prepare(target: &Value) -> Value {
    res_value(Rc::new(prepare_res(target)), Some(Value::Null))
}

/// The `FuzzySort` singleton of the `Impulse` module.
pub static TYPE: crate::impulse::Type = crate::impulse::Type {
    name: "FuzzySort",
    methods: &[
        ("go", go),
        ("prepare", |_, _, a| {
            Ok(prepare(&a.first().cloned().unwrap_or_default()))
        }),
    ],
    init: None,
};

#[cfg(all(test, feature = "interp"))]
mod tests {
    use super::*;
    use crate::eval::load_library;

    /// More distinct searches and string targets than the caps, with results held across
    /// later searches (their `_indexes` stay shared with the prepared target) and `all`
    /// identity checks.
    const SRC: &str = r#"
var out = [];
var words = [];
for (var i = 0; i < 1500; i++) words.push('item ' + i + ' ' + 'abcdefghijklmnopqrstuvwxyz'[i % 26] + 'x' + (i * 7919 % 1000));
var show = x => x.target + '|' + x.score + '|' + x._score + '|' + x.indexes.join('.');
var held = Fuzzy.go('item 1', words.slice(0, 50), { limit: 5 });
var heldAll = Fuzzy.go('', words.slice(0, 3), { all: true });
for (var q = 0; q < 1300; q++) {
  var from = (q * 37) % 1480;
  var r = Fuzzy.go('i' + (q % 10) + ' ' + q, words.slice(from, from + 20), { limit: 3 });
  out.push(q + ':' + r.total + ':' + r.map(show).join(';'));
  if (q % 100 == 0) out.push('H ' + held.map(show).join(';'));
}
out.push('H ' + held.map(show).join(';'));
var cased = ['ΟΔΟΣ ΑΘΗΝΑΣ', 'Σίσυφος', 'İstanbul'];
out.push('C ' + ['οδος', 'ς', 'σ', 'i', 'ı'].map(q => Fuzzy.go(q, cased).map(show).join(';')).join('/'));
out.push('A ' + heldAll.map(x => show(x) + '|' + (x === Fuzzy.go('', [x.target], { all: true })[0])).join(';'));
var res = out.join('\n');
"#;

    fn run(fuzzy: Value) -> String {
        let mut it = Interp::default();
        let g = it.global.clone();
        crate::watchdog::set_timeout(None);
        it.define(&g, "Fuzzy", fuzzy);
        let body = crate::ir::lower_program(SRC).expect("parse");
        if let Err(e) = it.run(&body, &g) {
            let msg = match e {
                crate::eval::Ex::Throw(v) => it
                    .get(&v, "message")
                    .map(|m| m.to_js_string())
                    .unwrap_or_else(|_| v.to_js_string()),
                crate::eval::Ex::Short => "short-circuit".into(),
            };
            panic!("run: {msg}");
        }
        let v = g.borrow().vars.get("res").cloned().unwrap_or_default();
        v.to_js_string()
    }

    #[test]
    fn bounded_caches_answer_like_the_unbounded_js_original() {
        // The tree-walking interpreter needs more than the default test-thread stack.
        std::thread::Builder::new()
            .stack_size(256 << 20)
            .spawn(compare_with_js)
            .unwrap()
            .join()
            .unwrap();
    }

    fn compare_with_js() {
        // The JS library this module was written from, kept as the reference of the test.
        let src = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/jslibs/testdata/fuzzysort.js"
        );
        let realm = Interp::default().global;
        let (mut shared, mut errors) = Default::default();
        let mut libs = crate::eval::Libraries {
            shared: &mut shared,
            files: &mut Default::default(),
            modules: &|_| None,
            errors: &mut errors,
            scope: &mut || None,
        };
        let js = load_library(src.as_ref(), &realm, &mut libs).expect("fuzzysort.js");
        assert_eq!(errors, Vec::<String>::new());
        let want = run(js);
        assert_eq!(want.lines().count(), 1300 + 13 + 3);
        assert!(want.contains("|true"), "{}", want.lines().last().unwrap());
        let native = new_obj();
        for &(name, f) in TYPE.methods {
            set_own(&native, name, fn_value(Callable::Native(name, f)));
        }
        let got = run(Value::Obj(native));
        assert_eq!(got, want);
        let prepared = PREPARED.with(|c| c.borrow().len());
        let searches = SEARCHES.with(|c| c.borrow().len());
        assert!(prepared < 2 * PREPARED_SWEEP, "{prepared}");
        assert!(searches <= SEARCHES_MAX, "{searches}");
    }
}
