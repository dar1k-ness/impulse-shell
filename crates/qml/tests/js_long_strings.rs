//! Calls on one long string cost work in proportion to its length, however many of them a
//! loop makes: `while (re.exec(text))`, `for (i = 0; i < text.length; i += w) text[i]`. A JS
//! string is indexed by UTF-16 code unit (ECMA-262 §6.1.4) while a well-formed one is held
//! as UTF-8, and a call that reads the whole string to get at its units makes such a loop
//! cost the square of the length: the notes overlay's line parser was interrupted by the
//! watchdog on a 580 KB note that QV4 6.12 parses in 3 ms. The work is counted, not timed
//! ([`work`]).
#![cfg(feature = "interp")]

#[path = "support/work.rs"]
mod work;

use qml::runtime::Runtime;
use qml::value::Value;
use std::rc::Rc;
use work::Work;

/// `text(lines, kind)` builds `lines` lines of one width, every fourth a list entry (`- `);
/// kind 1 is not ASCII, kind 2 ends in an unpaired surrogate as well (a string held as
/// UTF-16). Every other function works on such a text `s` of `n` lines `w` code units wide
/// in a loop of about `n` turns.
const SRC: &str = r#"import QtQuick
Item {
    function text(lines, kind) {
        var out = [], word = kind ? "запись в этой заметке, №" : "an entry of the note, number";
        for (var i = 0; i < lines; i++)
            out.push((i % 4 ? "  " : "- ") + word + " " + String(100000 + i).slice(1));
        return out.join("\n") + (kind === 2 ? "\ud800" : "");
    }
    // The loop of qml/modules/rusty/overlay/notes/NotesContent.qml (updateCopyListEntries).
    function notes(textValue) {
        const lineRegex = /(.*?)(\r?\n|$)/g;
        let match = null;
        const parsed = [];
        while ((match = lineRegex.exec(textValue)) !== null) {
            const lineText = match[1];
            const newlineText = match[2];
            const lineStart = match.index;
            const lineEnd = lineStart + lineText.length;
            const bulletMatch = lineText.match(/^\s*-\s+(.*\S)\s*$/);
            if (bulletMatch) {
                parsed.push({
                    content: bulletMatch[1].trim(),
                    start: lineStart,
                    end: lineEnd
                });
            }
            if (newlineText === "")
                break;
        }
        return parsed.length;
    }
    function exec(s) { var re = /\n/g, k = 0; while (re.exec(s)) k++; return k; }
    function test(s) { var re = /\n/g, k = 0; while (re.test(s)) k++; return k; }
    function match(s, w, n) { var k = 0; for (var i = 0; i < n; i++) if (s.match(/^-/)) k++; return k; }
    function search(s, w, n) { var k = 0; for (var i = 0; i < n; i++) k += s.search(/ /); return k; }
    function matchGlobal(s) { return s.match(/\n/g).length; }
    function matchAll(s) { var k = 0; for (const m of s.matchAll(/\n/g)) k++; return k; }
    function replace(s) { return s.replace(/\n/g, "$&$&").length - s.length; }
    function replaceCall(s) { var k = 0; s.replace(/\n/g, function (m) { k++; return m; }); return k; }
    function split(s) { return s.split(/\n/).length; }
    function length(s, w) { var k = 0; for (var i = 0; i < s.length; i += w) k++; return k; }
    function index(s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (s[i] === "-") k++; return k; }
    function charAt(s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (s.charAt(i) === "-") k++; return k; }
    function at(s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (s.at(i) === "-") k++; return k; }
    function charCodeAt(s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (s.charCodeAt(i) === 45) k++; return k; }
    function codePointAt(s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (s.codePointAt(i) === 45) k++; return k; }
    function substring(s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (s.substring(i, i + 1) === "-") k++; return k; }
    function substr(s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (s.substr(i, 1) === "-") k++; return k; }
    function slice(s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (s.slice(i, i + 1) === "-") k++; return k; }
    function startsWith(s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (s.startsWith("-", i)) k++; return k; }
    function endsWith(s, w) { var k = 0, l = s.length; for (var i = w; i < l; i += w) if (s.endsWith("\n", i)) k++; return k; }
    function indexOf(s) { var k = 0, p = 0; while ((p = s.indexOf("\n", p) + 1) > 0) k++; return k; }
    function lastIndexOf(s) { var k = 0, p = s.length; while ((p = s.lastIndexOf("\n", p) - 1) > -2) k++; return k; }
    function has(s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (Object.prototype.hasOwnProperty.call(s, i)) k++; return k; }
    function descriptor(s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (Object.getOwnPropertyDescriptor(s, i).value === "-") k++; return k; }
    function includes(s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (s.includes("-", i)) k++; return k; }
}"#;

/// What a function of [`SRC`] returns on a text of `n` lines.
type Count = fn(usize) -> usize;

/// Each function of [`SRC`] with its result.
const CALLS: [(&str, Count); 26] = [
    ("notes", |n| n / 4),
    ("exec", |n| n - 1),
    ("test", |n| n - 1),
    ("match", |n| n),
    ("search", |n| n),
    ("matchGlobal", |n| n - 1),
    ("matchAll", |n| n - 1),
    ("replace", |n| n - 1),
    ("replaceCall", |n| n - 1),
    ("split", |n| n),
    ("length", |n| n),
    ("index", |n| n / 4),
    ("charAt", |n| n / 4),
    ("at", |n| n / 4),
    ("charCodeAt", |n| n / 4),
    ("codePointAt", |n| n / 4),
    ("substring", |n| n / 4),
    ("substr", |n| n / 4),
    ("slice", |n| n / 4),
    ("startsWith", |n| n / 4),
    ("endsWith", |n| n - 1),
    ("indexOf", |n| n - 1),
    ("lastIndexOf", |n| n - 1),
    ("has", |n| n),
    ("descriptor", |n| n / 4),
    // The last entry starts the last but three of the lines.
    ("includes", |n| n - 3),
];

struct Text {
    rt: Runtime,
    root: usize,
    s: Value,
    lines: usize,
    width: usize,
}

impl Text {
    fn new(lines: usize, kind: u8) -> Self {
        let mut rt = Runtime::new(None);
        let root = rt.load_str(SRC).unwrap();
        let args = vec![Value::Num(lines as f64), Value::Num(kind.into())];
        let s = rt.call_method(root, "text", args).unwrap().unwrap();
        let width = s.to_js_string().lines().next().unwrap().chars().count() + 1;
        Self {
            rt,
            root,
            s,
            lines,
            width,
        }
    }

    /// The result of function `f` on a copy of the text nothing has read yet, and the work
    /// of the call.
    fn call(&mut self, f: &str) -> (Result<String, String>, Work) {
        let s = match &self.s {
            Value::Str(s) => Value::Str(Rc::from(&**s)),
            Value::Str16(u) => Value::Str16(Rc::from(&**u)),
            v => unreachable!("the text is {v:?}"),
        };
        let args = vec![
            s,
            Value::Num(self.width as f64),
            Value::Num(self.lines as f64),
        ];
        let start = Work::now();
        let r = self.rt.call_method(self.root, f, args).unwrap();
        let work = Work::now().since(start);
        (r.map(|v| v.to_js_string()), work)
    }
}

/// The note of the report: 16000 lines, 580 KB. The watchdog, which interrupted its parser,
/// is on.
#[test]
fn notes_parser_reads_a_580_kb_note() {
    let mut note = Text::new(16000, 0);
    let size = note.s.to_js_string().len();
    assert!((580_000..600_000).contains(&size), "{size} bytes");
    let (entries, work) = note.call("notes");
    println!("notes parser, {size} bytes in 16000 lines: {work}");
    assert_eq!(entries, Ok("4000".into()));
    assert_eq!(note.rt.errors(), Vec::<String>::new());
}

/// A text 16 times as long costs each loop about 16 times the work: under 64 times, where
/// copying or reading the whole string on each turn costs 256 times. No clock takes part:
/// the watchdog is off, so a loop that costs the square is counted to its end, which on
/// texts of this length takes it a minute.
/// The count does not see a loop that reads the whole string without allocating and
/// outside the four counted functions (`utf16::count`, `utf16::encode`, `JsStr::units`,
/// `JsStr::len`).
#[test]
fn calls_on_a_long_string_cost_its_length() {
    const SHORT: usize = 256;
    const GROWTH: usize = 16;
    qml::watchdog::set_timeout(None);
    let mut slow = Vec::new();
    for (kind, name) in ["ascii", "not ascii", "unpaired surrogate"]
        .into_iter()
        .enumerate()
    {
        let mut short = Text::new(SHORT, kind as u8);
        let mut long = Text::new(SHORT * GROWTH, kind as u8);
        for (f, result) in CALLS {
            let work = |text: &mut Text| {
                let (r, work) = text.call(f);
                assert_eq!(r, Ok(result(text.lines).to_string()), "{f} on {name}");
                work
            };
            let (a, b) = (work(&mut short), work(&mut long));
            println!("{name:18} {f:12} {a} -> {b}");
            if let Some(what) = b.grew_faster_than(a, GROWTH) {
                slow.push(format!("{f} on {name}: {a} -> {b}, too much {what}"));
            }
        }
        assert_eq!(long.rt.errors(), Vec::<String>::new());
    }
    assert!(
        slow.is_empty(),
        "{GROWTH} times the text cost {} times the work or more:\n{}",
        4 * GROWTH,
        slow.join("\n")
    );
}
