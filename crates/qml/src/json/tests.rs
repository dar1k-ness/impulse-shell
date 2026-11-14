//! Equivalence of the serde_json code with the hand-written code it replaced ([`legacy`]) and,
//! where they differ, with QJSEngine (V4, the original's engine; PySide6, ignored test) and V8.
use super::legacy;
use crate::eval::{Ex, Interp, new_obj, set_own};
use crate::value::Value;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const NUMS: &[f64] = &[
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.1,
    0.3,
    1.5,
    -7.25,
    1e21,
    1e-7,
    5e-324,
    1.7976931348623157e308,
    9007199254740993.0,
    9e15,
    1e15 + 0.5,
    1793217195000.0,
    123456789.125,
    1e-6,
    0.000001234,
    2.5e-5,
    1e300,
    f64::NAN,
    f64::INFINITY,
    f64::NEG_INFINITY,
    100.0,
    4294967296.0,
];
const CHARS: &[char] = &[
    'a', 'Z', '0', ' ', '"', '\\', '/', '\n', '\r', '\t', '\u{8}', '\u{c}', '\u{0}', '\u{1}',
    '\u{1f}', '\u{7f}', '\u{80}', 'é', 'я', '中', '\u{2028}', '\u{feff}', '🙂', '\u{fffd}', ':',
    ',',
];

fn num(r: &mut Rng) -> f64 {
    match r.below(4) {
        0 => NUMS[r.below(NUMS.len() as u64) as usize],
        1 => (r.below(2_000_000) as f64) - 1_000_000.0,
        2 => f64::from_bits(r.next()),
        _ => (r.below(1 << 20) as f64) / (1u64 << r.below(30)) as f64,
    }
}

fn string(r: &mut Rng) -> String {
    let n = r.below(8);
    (0..n)
        .map(|_| CHARS[r.below(CHARS.len() as u64) as usize])
        .collect()
}

/// A random JS value; `extra` adds the non-JSON values stringify must drop or map.
fn value(r: &mut Rng, depth: u32, extra: bool) -> Value {
    let kinds = if depth >= 4 { 4 } else { 6 };
    match r.below(kinds + if extra { 2 } else { 0 }) {
        0 => Value::Null,
        1 => Value::Bool(r.below(2) == 0),
        2 => Value::Num(num(r)),
        3 => Value::str(&string(r)),
        4 => Value::array(
            (0..r.below(5))
                .map(|_| value(r, depth + 1, extra))
                .collect(),
        ),
        5 => {
            let o = new_obj();
            for _ in 0..r.below(5) {
                set_own(&o, &string(r), value(r, depth + 1, extra));
            }
            Value::Obj(o)
        }
        6 => Value::Undefined,
        _ => {
            if r.below(2) == 0 {
                Value::Native(0)
            } else {
                Value::Item(0)
            }
        }
    }
}

/// Exact structure: key order, number bits, string contents.
fn dump(v: &Value, out: &mut String) {
    match v {
        Value::Num(n) => out.push_str(&format!("n{:x}", n.to_bits())),
        Value::Str(s) => out.push_str(&format!("s{s:?}")),
        Value::Obj(o) => {
            let b = o.borrow();
            if let Some(a) = &b.array {
                out.push('[');
                for x in a {
                    dump(x, out);
                    out.push(',');
                }
                out.push(']');
            } else {
                out.push('{');
                for k in &b.keys {
                    out.push_str(&format!("{k:?}:"));
                    dump(&b.props[k], out);
                    out.push(',');
                }
                out.push('}');
            }
        }
        v => out.push_str(&format!("{v:?}")),
    }
}

fn d(v: &Value) -> String {
    let mut s = String::new();
    dump(v, &mut s);
    s
}

fn old_parse(s: &str) -> Option<String> {
    legacy::parse_json(s).map(|v| d(&v))
}

fn new_parse(s: &str) -> Option<String> {
    super::parse(s).ok().map(|v| d(&v))
}

/// The hand-written writer printed 17-digit halfway cases with Rust's `{}` (round half up:
/// `0.22441482543945313`); V8's Number::toString, and ryu-js, pick the even digit
/// (`0.22441482543945312`). Equal apart from such numbers (same length, same f64).
fn same_but_v8_ties(new: &str, old: &str) -> bool {
    let (a, b) = (new.as_bytes(), old.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] == b[i] {
            i += 1;
            continue;
        }
        let (mut s, mut e) = (i, i);
        while s > 0 && (a[s - 1].is_ascii_digit() || a[s - 1] == b'.') {
            s -= 1;
        }
        while e < a.len() && (a[e].is_ascii_digit() || matches!(a[e], b'.' | b'e' | b'+' | b'-')) {
            e += 1;
        }
        let f = |x: &[u8]| std::str::from_utf8(&x[s..e]).ok()?.parse::<f64>().ok();
        if f(a).is_none() || f(a).map(f64::to_bits) != f(b).map(f64::to_bits) {
            return false;
        }
        i = e;
    }
    true
}

fn assert_stringify_eq(
    new: Result<Option<String>, String>,
    old: Result<Option<String>, String>,
    what: &str,
) {
    match (&new, &old) {
        (Ok(Some(a)), Ok(Some(b))) if a != b => assert!(same_but_v8_ties(a, b), "{what}\n{a}\n{b}"),
        _ => assert_eq!(new, old, "{what}"),
    }
}

fn old_stringify(it: &mut Interp, v: &Value, indent: &str) -> Result<Option<String>, String> {
    legacy::json_str(it, v, indent, "", 0).map_err(|e| err(it, e))
}

fn new_stringify(it: &mut Interp, v: &Value, indent: &str) -> Result<Option<String>, String> {
    super::stringify(it, v, indent).map_err(|e| err(it, e))
}

fn err(it: &mut Interp, e: Ex) -> String {
    match e {
        Ex::Throw(v) => it.get(&v, "message").unwrap().to_js_string(),
        Ex::Short => "short".into(),
    }
}

const INDENTS: &[&str] = &["", " ", "  ", "\t", "--", "          "];

#[test]
fn stringify_equals_legacy_on_random_values() {
    let mut r = Rng(0x2545f4914f6cdd1d);
    let mut it = Interp::default();
    for n in 0..20_000 {
        let v = value(&mut r, 0, true);
        let ind = INDENTS[n % INDENTS.len()];
        let (new, old) = (
            new_stringify(&mut it, &v, ind),
            old_stringify(&mut it, &v, ind),
        );
        assert_stringify_eq(new, old, &d(&v));
    }
}

#[test]
fn stringify_numbers_equal_legacy() {
    let mut r = Rng(0x9e3779b97f4a7c15);
    let mut it = Interp::default();
    let mut check = |n: f64| {
        let v = Value::Num(n);
        let (new, old) = (
            new_stringify(&mut it, &v, ""),
            old_stringify(&mut it, &v, ""),
        );
        assert_stringify_eq(new, old, &format!("{n:e}"));
    };
    NUMS.iter().copied().for_each(&mut check);
    for _ in 0..200_000 {
        check(num(&mut r));
    }
    for e in -330..310 {
        check(10f64.powi(e));
        check(1.5 * 10f64.powi(e));
    }
}

/// The halfway cases where the hand-written writer differed, with V8's output.
#[test]
fn stringify_numbers_like_v8_where_legacy_differed() {
    let mut it = Interp::default();
    for (n, v8) in [
        (2.6583023071289063, "2.6583023071289062"),
        (0.22441482543945313, "0.22441482543945312"),
    ] {
        assert_eq!(
            new_stringify(&mut it, &Value::Num(n), ""),
            Ok(Some(v8.into()))
        );
    }
}

#[test]
fn stringify_errors_equal_legacy() {
    let mut it = Interp::default();
    let o = new_obj();
    set_own(&o, "self", Value::Obj(o.clone()));
    let v = Value::Obj(o);
    // The legacy writer recursed into the cycle until its depth limit; a cycle is reported
    // as one, like V8.
    assert_eq!(
        old_stringify(&mut it, &v, ""),
        Err("JSON.stringify: structure too deep".into())
    );
    assert_eq!(
        new_stringify(&mut it, &v, ""),
        Err("Cannot convert circular structure to JSON".into())
    );
    // Deep but acyclic is written up to the JS depth limit and throws `RangeError` beyond, as
    // in V4; the legacy writer stopped at 256 levels.
    let mut deep = Value::Num(1.0);
    for _ in 0..300 {
        deep = Value::array(vec![deep]);
    }
    let want = format!("{}1{}", "[".repeat(300), "]".repeat(300));
    assert_eq!(new_stringify(&mut it, &deep, ""), Ok(Some(want)));
    for _ in 300..=crate::eval::MAX_JS_DEPTH {
        deep = Value::array(vec![deep]);
    }
    assert_eq!(
        new_stringify(&mut it, &deep, ""),
        Err("Maximum call stack size exceeded".into())
    );
    // A shared (not cyclic) object is written each time it appears.
    let shared = Value::Obj(new_obj());
    let two = Value::array(vec![shared.clone(), shared]);
    assert_eq!(new_stringify(&mut it, &two, ""), Ok(Some("[{},{}]".into())));
}

#[test]
fn parse_equals_legacy_on_valid_json() {
    let mut r = Rng(0xdeadbeefcafef00d);
    let mut it = Interp::default();
    for n in 0..20_000 {
        let v = value(&mut r, 0, false);
        let ind = ["", " ", "  ", "\t", "    "][n % 5];
        let Ok(Some(text)) = old_stringify(&mut it, &v, ind) else {
            continue;
        };
        let new = new_parse(&text);
        assert!(new.is_some(), "{text}");
        assert_eq!(new, old_parse(&text), "{text}");
        // Whitespace JSON allows anywhere between tokens.
        let spaced = format!(" \r\n\t{text}\n ");
        assert_eq!(new_parse(&spaced), new, "{spaced:?}");
    }
}

/// Hand-picked texts both parsers read the same way.
#[test]
fn parse_equals_legacy_on_edge_cases() {
    for s in [
        r#""\u00e9\ud83d\ude00\/\b\f\n\r\t\"\\""#,
        r#""\uD83D\uDE00 upper hex""#,
        r#""\ud800""#,
        r#""a\udc00b\ud800\ud800\udc00""#,
        r#"{"a":1,"b":2,"a":3}"#,
        r#"{"":{"":[]}}"#,
        "-0",
        "0",
        "-0.0",
        "0e-400",
        "1e-310",
        "3e-324",
        "1.7976931348623158e308",
        "123456789012345678901234567890",
        "9007199254740993",
        "18446744073709551616",
        "0.1e1",
        "1.7976931348623157e308",
        "4.9406564584124654e-324",
        "2.2250738585072011e-308",
        "[1,[2,[3,{\"k\":[true,false,null]}]]]",
        "\"привет 🙂\"",
        " [ ] ",
        "{ }",
    ] {
        assert!(new_parse(s).is_some(), "{s}");
        assert_eq!(new_parse(s), old_parse(s), "{s}");
    }
    let deep = |n: usize| "[".repeat(n) + &"]".repeat(n);
    for n in [1, 128, 129, 512, 513, 514, 2000] {
        assert_eq!(
            new_parse(&deep(n)).is_some(),
            old_parse(&deep(n)).is_some(),
            "{n}"
        );
    }
}

/// Numbers that round to ±Infinity, or a non-zero mantissa that rounds to 0: QJSEngine (V4,
/// the original's engine) throws a SyntaxError; V8 and the hand-written parser gave ±Infinity / 0.
#[test]
fn parse_rejects_out_of_range_numbers_like_v4() {
    for s in [
        "1E400",
        "-1e309",
        "[1e999]",
        "1.7976931348623159e308",
        "1e-400",
        "-1e-400",
        "2e-324",
        "2.4703282292062327e-324",
    ] {
        assert_eq!(new_parse(s), None, "{s}");
        assert!(old_parse(s).is_some(), "{s}");
    }
    for (s, n) in [
        ("2.4703282292062328e-324", 5e-324),
        ("0.0e999", 0.0),
        ("-0e5", -0.0),
    ] {
        let Ok(Value::Num(v)) = super::parse(s) else {
            panic!("{s}")
        };
        assert_eq!(v.to_bits(), f64::to_bits(n), "{s}");
    }
}

/// A JSON object whose key is serde_json's private number token is still an object.
#[test]
fn parse_number_token_key_is_an_object() {
    let s = r#"{"$serde_json::private::Number":"1e5"}"#;
    assert_eq!(new_parse(s), old_parse(s));
    assert!(matches!(super::parse(s), Ok(Value::Obj(_))));
}

/// Texts JSON (ECMA-404) and V8/V4 reject: both parsers fail.
#[test]
fn parse_rejects_like_legacy() {
    for s in [
        "",
        " ",
        "[",
        "]",
        "{",
        "[1,]",
        "{\"a\":1,}",
        "{a:1}",
        "{'a':1}",
        "'a'",
        "\"a",
        "[1 2]",
        "{\"a\" 1}",
        "tru",
        "nul",
        "True",
        "NaN",
        "Infinity",
        "-",
        "1e",
        "1e+",
        "--1",
        "1 2",
        "[]x",
        "{}}",
        "\"\\u12\"",
        "\"\\u12G4\"",
        "/*c*/1",
        "1//c",
        "\u{feff}1",
    ] {
        assert_eq!(new_parse(s), None, "{s:?}");
        assert_eq!(old_parse(s), None, "{s:?}");
    }
}

/// Where the hand-written parser was laxer than JSON: serde_json rejects these, as V8 and
/// Qt's V4 do (`ignored_matches_node` checks them against V8).
pub(super) const LEGACY_LAX: &[&str] = &[
    "+1",
    "01",
    "-01",
    ".",
    "-.",
    ".e5",
    "1.e",
    "..5",
    "1.5.",
    "-.e1",
    "-01.",
    "\"\\x\"",
    "\"\\a\"",
    "\"\\'\"",
    "\"a\u{1}b\"",
    "\"tab\there\"",
    "\"nl\nhere\"",
    "0x10",
    "1e5.5",
];

/// Numbers with digits on one side of the point only: JSON forbids them, V4 (and the
/// hand-written parser) read them.
const V4_DOT_NUMBERS: &[&str] = &[
    "1.",
    ".5",
    "-.5",
    "1.e5",
    "1.E+2",
    "0.",
    "-0.",
    "-.0e-1",
    " .5 ",
    "[.5,1.]",
    r#"{"a":.5,"b":"x.5"}"#,
];

#[test]
fn parse_accepts_v4_dot_numbers() {
    for s in V4_DOT_NUMBERS {
        assert!(new_parse(s).is_some(), "{s:?}");
        assert_eq!(new_parse(s), old_parse(s), "{s:?}");
    }
    assert_eq!(new_parse(r#"["a.",".b"]"#), old_parse(r#"["a.",".b"]"#));
}

#[test]
fn parse_rejects_what_legacy_accepted_but_json_forbids() {
    for s in LEGACY_LAX {
        assert_eq!(new_parse(s), None, "{s:?}");
    }
}

#[test]
#[cfg(feature = "interp")]
fn parse_errors_become_syntax_errors() {
    let body = crate::ir::lower_program(
        "try { JSON.parse('{\"a\":'); return 'no' } catch (e) { return e.name + ' ' + (e instanceof SyntaxError) }",
    )
    .unwrap();
    let mut it = Interp::default();
    let g = it.global.clone();
    let v = it.run(&body, &g).ok().unwrap();
    assert_eq!(v.to_js_string(), "SyntaxError true");
}

fn adapter_world(r: &mut Rng) -> (crate::runtime::Runtime, usize, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("ri-json-eq-{}-{}", std::process::id(), r.next()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("c.json");
    let mut rt = crate::runtime::Runtime::new(None);
    rt.load_str(&format!(
        "import QtQuick\nimport Rustshell.Io\nItem {{\n\
         FileViewInternal {{ id: fv; property string path: \"{}\"\n\
         JsonAdapter {{ property var a; property var b; property list<var> l: []\n\
         property string s: \"x\"; property real f: 0.5; property bool t: true\n\
         property JsonObject n: JsonObject {{ property var c; property list<string> m: [] }} }} }} }}",
        path.display()
    ))
    .unwrap();
    rt.settle();
    let a = {
        let w = rt.world.borrow();
        (0..w.items.len())
            .find(|&i| w.items[i].types.iter().any(|t| t == "JsonAdapter"))
            .unwrap()
    };
    (rt, a, dir)
}

#[test]
fn adapter_text_equals_legacy() {
    let mut r = Rng(0x1234567887654321);
    let (rt, a, dir) = adapter_world(&mut r);
    let n = {
        let w = rt.world.borrow();
        match w.items[a].props.get("n").map(|p| p.value.clone()) {
            Some(Value::Item(n)) => n,
            v => panic!("{v:?}"),
        }
    };
    for _ in 0..5_000 {
        let mut w = rt.world.borrow_mut();
        for (item, prop) in [(a, "a"), (a, "b"), (n, "c"), (a, "f")] {
            let v = if prop == "f" {
                Value::Num(num(&mut r))
            } else {
                value(&mut r, 0, true)
            };
            w.write(item, prop, v);
        }
        let mut old = String::new();
        legacy::adapter::json_write(&w, &Value::Item(a), 0, &mut old);
        old.push('\n');
        assert_eq!(super::adapter_text(&w, a), old);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Texts for the engine comparisons: legacy-lax texts, `extra`, random values (without
/// integer-index keys: V8 and V4 list those first, JS objects here keep insertion order for
/// every key, the interpreter's object model) and numbers as text in several notations.
fn reference_texts(extra: &[&str]) -> Vec<String> {
    let mut r = Rng(0x5eed);
    let mut it = Interp::default();
    let mut texts: Vec<String> = LEGACY_LAX
        .iter()
        .chain(extra)
        .map(|s| s.to_string())
        .collect();
    for _ in 0..3_000 {
        let v = value(&mut r, 0, false);
        if let Ok(Some(t)) = new_stringify(&mut it, &v, "") {
            let index_key = t.split('"').collect::<Vec<_>>().windows(2).any(|w| {
                !w[0].is_empty()
                    && w[0].bytes().all(|b| b.is_ascii_digit())
                    && w[1].starts_with(':')
            });
            if !index_key {
                texts.push(t);
            }
        }
    }
    for n in NUMS.iter().copied().chain((0..3_000).map(|_| num(&mut r))) {
        texts.extend([format!("{n:e}"), format!("{n:?}"), format!("[{n:E}]")]);
    }
    texts.retain(|t| !t.contains("inf") && !t.contains("NaN"));
    texts
}

/// Feeds each text (one JSON string per line) to `cmd`, which prints per line
/// `[JSON.stringify(v), JSON.stringify(v, null, 2)]` for `v = JSON.parse(text)`, or `null`.
fn assert_matches_engine(cmd: &str, args: &[&str], texts: &[String]) {
    use std::io::Write;
    let mut it = Interp::default();
    let mut child = std::process::Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect(cmd);
    let mut input = String::new();
    for t in texts {
        // ASCII only (\uXXXX for the rest), whatever the engine's stdin encoding.
        for c in serde_json::to_string(t).unwrap().chars() {
            if c.is_ascii() {
                input.push(c);
            } else {
                for u in c.encode_utf16(&mut [0; 2]) {
                    input.push_str(&format!("\\u{u:04x}"));
                }
            }
        }
        input.push('\n');
    }
    let mut stdin = child.stdin.take().unwrap();
    let feed = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let out = child.wait_with_output().unwrap();
    feed.join().unwrap();
    let lines: Vec<&str> = std::str::from_utf8(&out.stdout).unwrap().lines().collect();
    assert_eq!(lines.len(), texts.len());
    let mut bad = Vec::new();
    for (t, l) in texts.iter().zip(lines) {
        let want: Option<(String, String)> =
            serde_json::from_str(l).unwrap_or_else(|e| panic!("{t:?} -> {l:?}: {e}"));
        let got = super::parse(t).ok().map(|v| {
            (
                new_stringify(&mut it, &v, "").unwrap().unwrap(),
                new_stringify(&mut it, &v, "  ").unwrap().unwrap(),
            )
        });
        if got != want {
            bad.push(format!("{t:?}\n  ours: {got:?}\n  {cmd}: {want:?}"));
        }
    }
    let n = bad.len();
    bad.truncate(20);
    assert!(n == 0, "{n} of {} differ:\n{}", texts.len(), bad.join("\n"));
}

/// The halfway digits, edge numbers and duplicate keys checked against both engines.
const ENGINE_EXTRA: &[&str] = &[
    "-0",
    "0.22441482543945313",
    "2.6583023071289063",
    "9007199254740993",
    "9007199254740992",
    "1.7976931348623158e308",
    "2.4703282292062328e-324",
    r#"{"a":1,"b":2,"a":3}"#,
    r#"{"b":1,"a":2,"b":3}"#,
];

/// QJSEngine (V4) is the original's engine, so the reference for parity:
/// `cargo test --features live -p qml json::tests::ignored_matches_qjsengine -- --exact` (needs PySide6 6.12).
#[test]
#[cfg_attr(
    not(feature = "live"),
    ignore = "live: --features live, docs/benchmarks.md"
)]
fn ignored_matches_qjsengine() {
    let script = r#"
import sys
from PySide6 import __version__
from PySide6.QtCore import QCoreApplication
from PySide6.QtQml import QJSEngine
assert __version__.startswith("6.12"), __version__
app = QCoreApplication(sys.argv[:1])
e = QJSEngine()
f = e.evaluate(r'''(function (line) {
  var t = JSON.parse(line), o;
  try { var v = JSON.parse(t); o = [JSON.stringify(v), JSON.stringify(v, null, 2)]; } catch (x) { o = null; }
  return JSON.stringify(o).replace(/[\u007f-\uffff]/g, function (c) {
    return '\\u' + ('000' + c.charCodeAt(0).toString(16)).slice(-4);
  });
})''')
for line in sys.stdin:
    line = line.rstrip("\n")
    if line:
        e.globalObject().setProperty("line", line)
        print(f.call([e.globalObject().property("line")]).toString())
"#;
    let mut extra = ENGINE_EXTRA.to_vec();
    // V4 throws on these (V8 does not): see parse_rejects_out_of_range_numbers_like_v4.
    extra.extend([
        "1E400",
        "-1e309",
        "1.7976931348623159e308",
        "1e-400",
        "-1e-400",
        "2e-324",
    ]);
    extra.extend(V4_DOT_NUMBERS);
    assert_matches_engine("python3", &["-c", script], &reference_texts(&extra));
}

/// V8 as a second reference (node is in tools/testenv/Containerfile).
/// Not "\ud800" (both engines keep lone surrogates, our strings are UTF-8) nor the
/// out-of-range numbers V8 accepts and V4 rejects.
#[test]
fn matches_node() {
    if !crate::testenv::require("node") {
        return;
    }
    let script = "const l=require('fs').readFileSync(0,'utf8').split('\\n').filter(x=>x).map(x=>JSON.parse(x));\
                  for(const t of l){let o;try{const v=JSON.parse(t);o=[JSON.stringify(v),JSON.stringify(v,null,2)]}catch(e){o=null}\
                  console.log(JSON.stringify(o))}";
    assert_matches_engine("node", &["-e", script], &reference_texts(ENGINE_EXTRA));
}
