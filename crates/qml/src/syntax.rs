//! org.kde.syntaxhighlighting's `Repository` and `SyntaxHighlighter`: KSyntaxHighlighting's
//! own definitions and themes (the XML and JSON files of 6.31, see `syntax/data/LICENSES.md`)
//! read by [`kate`], which follows the library rule by rule. Highlighting runs on the work pool.
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex, OnceLock};

use crate::runtime::World;
use crate::value::Value;

pub mod kate;
mod pcre;

/// A file of `syntax/data`, deflated by build.rs: the binary holds the data packed.
macro_rules! packed {
    ($file:literal) => {
        include_bytes!(concat!(env!("OUT_DIR"), "/syntax/", $file))
    };
}

/// The text of a packed file, unpacked once.
fn unpacked(packed: &'static [u8]) -> &'static str {
    static TEXTS: OnceLock<Mutex<HashMap<usize, &'static str>>> = OnceLock::new();
    let mut texts = TEXTS.get_or_init(Default::default).lock().unwrap();
    texts.entry(packed.as_ptr() as usize).or_insert_with(|| {
        let mut text = String::new();
        flate2::read::DeflateDecoder::new(packed)
            .read_to_string(&mut text)
            .expect("build.rs packed this file");
        text.leak()
    })
}

type Entry = (&'static str, &'static [&'static str], &'static [u8]);

/// The definitions taken from KSyntaxHighlighting: (name, alternative names, packed XML). The
/// languages that come in AI answers and the definitions those include.
const DEFINITIONS: &[Entry] = &[
    ("Bash", &[], packed!("syntax/bash.xml")),
    ("C", &[], packed!("syntax/c.xml")),
    ("CMake", &[], packed!("syntax/cmake.xml")),
    ("C++", &["CPP"], packed!("syntax/cpp.xml")),
    ("C#", &["CS", "CSharp"], packed!("syntax/cs.xml")),
    ("CSS", &[], packed!("syntax/css.xml")),
    ("Diff", &[], packed!("syntax/diff.xml")),
    ("Dockerfile", &[], packed!("syntax/dockerfile.xml")),
    ("Go", &[], packed!("syntax/go.xml")),
    ("HTML", &[], packed!("syntax/html.xml")),
    ("Java", &[], packed!("syntax/java.xml")),
    ("JavaScript", &["JS"], packed!("syntax/javascript.xml")),
    ("JSON", &[], packed!("syntax/json.xml")),
    ("Kotlin", &["KT"], packed!("syntax/kotlin.xml")),
    ("LaTeX", &[], packed!("syntax/latex.xml")),
    ("Lua", &[], packed!("syntax/lua.xml")),
    ("Makefile", &[], packed!("syntax/makefile.xml")),
    ("Markdown", &[], packed!("syntax/markdown.xml")),
    ("Nix", &[], packed!("syntax/nix.xml")),
    ("Python", &["Py"], packed!("syntax/python.xml")),
    ("QML", &[], packed!("syntax/qml.xml")),
    ("Ruby", &["RB", "JRuby"], packed!("syntax/ruby.xml")),
    ("Rust", &["RS"], packed!("syntax/rust.xml")),
    ("SQL", &[], packed!("syntax/sql.xml")),
    ("Swift", &[], packed!("syntax/swift.xml")),
    ("TOML", &[], packed!("syntax/toml.xml")),
    ("TypeScript", &["TS"], packed!("syntax/typescript.xml")),
    ("XML", &[], packed!("syntax/xml.xml")),
    ("YAML", &["YML"], packed!("syntax/yaml.xml")),
    ("Alerts", &[], packed!("syntax/alert.xml")),
    ("Comments", &[], packed!("syntax/comments.xml")),
    ("Doxygen", &[], packed!("syntax/doxygen.xml")),
    ("DoxygenLua", &[], packed!("syntax/doxygenlua.xml")),
    ("GCCExtensions", &[], packed!("syntax/gcc.xml")),
    ("ISO C++", &[], packed!("syntax/isocpp.xml")),
    ("Javadoc", &[], packed!("syntax/javadoc.xml")),
    (
        "JavaScript React (JSX)",
        &["JSX"],
        packed!("syntax/javascript-react.xml"),
    ),
    ("Modelines", &[], packed!("syntax/modelines.xml")),
    (
        "Mustache/Handlebars (HTML)",
        &[],
        packed!("syntax/mustache.xml"),
    ),
    ("reStructuredText", &[], packed!("syntax/rest.xml")),
    ("SPDX-Comments", &[], packed!("syntax/spdx-comments.xml")),
];

/// The themes the shell names: (name, JSON).
const THEMES: &[(&str, &[u8])] = &[
    ("Monokai", packed!("themes/monokai.theme")),
    ("ayu Light", packed!("themes/ayu-light.theme")),
];

/// `Repository::definitionForName`: by name or alternative name, any case; of two
/// definitions with one name the later in the order of names wins.
fn entry(name: &str) -> Option<&'static Entry> {
    let low = name.to_lowercase();
    let mut found: Option<&'static Entry> = None;
    for d in DEFINITIONS {
        let named = d.0.to_lowercase() == low || d.1.iter().any(|a| a.to_lowercase() == low);
        if named && found.is_none_or(|f| f.0 < d.0) {
            found = Some(d);
        }
    }
    found
}

/// The name of the definition `Repository.definitionForName(name)` finds.
pub fn find(name: &str) -> Option<&'static str> {
    entry(name).map(|d| d.0)
}

/// `Repository.definitionForName(name)`: a `Definition` with `name`/`isValid`. An unknown
/// name gives an invalid definition that still carries the requested name (KSyntaxHighlighting
/// returns an empty one, which leaves AI-chat code block headers blank).
pub fn definition_value(name: &Value) -> Value {
    let req = match name {
        Value::Str(s) => s.to_string(),
        Value::Obj(o) => o
            .borrow()
            .props
            .get("name")
            .map(Value::to_js_string)
            .unwrap_or_default(),
        _ => String::new(),
    };
    let found = find(&req);
    let o = crate::eval::new_obj();
    let shown = found.map_or(req.clone(), str::to_string);
    crate::eval::set_own(&o, "name", Value::str(&shown));
    crate::eval::set_own(&o, "translatedName", Value::str(&shown));
    crate::eval::set_own(&o, "isValid", Value::Bool(found.is_some()));
    crate::eval::set_own(&o, "_request", Value::str(&req));
    Value::Obj(o)
}

/// The definition `name` names, loaded once.
fn definition(name: &str) -> Option<Arc<kate::Definition>> {
    type Loaded = HashMap<&'static str, Option<Arc<kate::Definition>>>;
    static LOADED: OnceLock<Mutex<Loaded>> = OnceLock::new();
    let e = entry(name)?;
    let mut loaded = LOADED.get_or_init(Default::default).lock().unwrap();
    loaded
        .entry(e.0)
        .or_insert_with(|| {
            let source = |n: &str| entry(n).and_then(|d| kate::parsed(unpacked(d.2)));
            kate::Definition::load(e.0, &source).map(Arc::new)
        })
        .clone()
}

/// What a theme sets for a text style or for one attribute of one definition.
#[derive(Clone, Copy, Default)]
struct ThemeStyle {
    color: Option<[u8; 4]>,
    background: Option<[u8; 4]>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
}

#[derive(Default)]
struct Theme {
    styles: HashMap<String, ThemeStyle>,
    custom: HashMap<String, HashMap<String, ThemeStyle>>,
}

fn theme_style(v: &serde_json::Value) -> ThemeStyle {
    let color = |k: &str| v.get(k).and_then(|c| c.as_str()).and_then(kate::color);
    let flag = |k: &str| v.get(k).and_then(|b| b.as_bool());
    ThemeStyle {
        color: color("text-color"),
        background: color("background-color"),
        bold: flag("bold"),
        italic: flag("italic"),
        underline: flag("underline"),
        strike: flag("strike-through"),
    }
}

fn theme(name: &str) -> Option<&'static Theme> {
    static LOADED: OnceLock<Vec<(&'static str, Theme)>> = OnceLock::new();
    let all = LOADED.get_or_init(|| {
        let load = |json: &str| {
            let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
            let styles = |v: Option<&serde_json::Value>| -> HashMap<String, ThemeStyle> {
                v.and_then(|s| s.as_object())
                    .map(|o| o.iter().map(|(k, s)| (k.clone(), theme_style(s))).collect())
                    .unwrap_or_default()
            };
            Theme {
                styles: styles(v.get("text-styles")),
                custom: v
                    .get("custom-styles")
                    .and_then(|c| c.as_object())
                    .map(|o| {
                        o.iter()
                            .map(|(k, s)| (k.clone(), styles(Some(s))))
                            .collect()
                    })
                    .unwrap_or_default(),
            }
        };
        THEMES.iter().map(|t| (t.0, load(unpacked(t.1)))).collect()
    });
    all.iter().find(|t| t.0 == name).map(|t| &t.1)
}

/// How a run of text is drawn: `SyntaxHighlighterPrivate::initTextFormat`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub color: [u8; 4],
    pub background: Option<[u8; 4]>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
}

impl Theme {
    /// `Format::textColor(theme)` and the rest of `Format` for a definition's format; `None`
    /// is the format of text no rule names.
    fn style(&self, format: Option<&kate::Format>) -> Style {
        let none = ThemeStyle::default();
        let plain = kate::Format::default();
        let f = format.unwrap_or(&plain);
        let name = if f.style.is_empty() {
            "Normal"
        } else {
            &f.style
        };
        let of = |n: &str| self.styles.get(n).copied().unwrap_or(none);
        let (base, normal) = (of(name), of("Normal"));
        let over = self
            .custom
            .get(&f.definition)
            .and_then(|d| d.get(&f.name))
            .copied()
            .unwrap_or(none);
        let opaque = |c: [u8; 4]| [c[0], c[1], c[2], 255];
        let background = over
            .background
            .map(opaque)
            .or(f.background)
            .or(base.background);
        Style {
            color: over
                .color
                .map(opaque)
                .or(f.color)
                .or(base.color)
                .unwrap_or([0; 4]),
            background: background.filter(|b| Some(*b) != normal.background),
            bold: over.bold.or(f.bold).or(base.bold).unwrap_or(false),
            italic: over.italic.or(f.italic).or(base.italic).unwrap_or(false),
            underline: over
                .underline
                .or(f.underline)
                .or(base.underline)
                .unwrap_or(false),
            strike: over.strike.or(f.strike).or(base.strike).unwrap_or(false),
        }
    }
}

/// A highlighted byte range of the text and how the theme draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub color: [u8; 4],
    pub background: Option<[u8; 4]>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
}

/// Highlight `text` with the definition `def` (a `definitionForName` request) in `theme`.
/// Every character of every line gets a span, as the library formats all of a line; a name
/// the repository does not know leaves the text in the theme's Normal colour.
pub fn highlight(text: &str, def: &str, theme_name: &str) -> Vec<Span> {
    let Some(theme) = theme(theme_name) else {
        return vec![];
    };
    let def = definition(def);
    let plain = theme.style(None);
    let styles: Vec<Style> = def
        .iter()
        .flat_map(|d| d.formats.iter().map(|f| theme.style(Some(f))))
        .collect();
    let mut out: Vec<Span> = vec![];
    let mut state = kate::State::default();
    let mut cache = kate::Cache::default();
    let mut base = 0;
    for line in text.split('\n') {
        let runs = match &def {
            Some(d) => d.line(line, &mut state, &mut cache),
            None if line.is_empty() => vec![],
            None => vec![(0, line.len(), None)],
        };
        for (from, to, format) in runs {
            let s = format.map_or(plain, |f| styles[f]);
            if from == to {
                continue;
            }
            let (start, end) = (base + from, base + to);
            match out.last_mut() {
                Some(l) if l.end == start && l.style() == s => l.end = end,
                _ => out.push(Span {
                    start,
                    end,
                    color: s.color,
                    background: s.background,
                    bold: s.bold,
                    italic: s.italic,
                    underline: s.underline,
                    strike: s.strike,
                }),
            }
        }
        base += line.len() + 1;
    }
    out
}

impl Span {
    pub fn style(&self) -> Style {
        Style {
            color: self.color,
            background: self.background,
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            strike: self.strike,
        }
    }
}

/// A finished highlight of one TextEdit's text.
#[derive(Debug)]
pub struct Highlight {
    pub text: String,
    pub spans: Vec<Span>,
}

type Key = (usize, String, String, String);

struct Done {
    hl: usize,
    key: Key,
    spans: Vec<Span>,
}

/// Highlighter jobs and their results, keyed by the TextEdit they colour.
#[derive(Default)]
pub struct State {
    chan: Option<(crate::svc::Sender<Done>, std::sync::mpsc::Receiver<Done>)>,
    /// Per highlighter: the key in flight and the last key applied.
    jobs: HashMap<usize, (Option<Key>, Option<Key>)>,
    pub results: HashMap<usize, Arc<Highlight>>,
}

fn str_of(w: &mut World, i: usize, k: &str) -> String {
    match w.read(i, k) {
        Some(Value::Str(s)) => s.to_string(),
        Some(Value::Obj(o)) => {
            let b = o.borrow();
            b.props
                .get("_request")
                .or_else(|| b.props.get("name"))
                .map(Value::to_js_string)
                .unwrap_or_default()
        }
        _ => String::new(),
    }
}

/// The spans for `te` while its text is still the highlighted one.
pub fn spans_for(w: &World, te: usize, text: &str) -> Option<Arc<Highlight>> {
    w.syntax
        .results
        .get(&te)
        .filter(|h| h.text == text && !h.spans.is_empty())
        .cloned()
}

/// Start a job for every highlighter whose (text, definition, theme) changed, apply finished
/// ones. One job per highlighter is in flight; a streaming message re-highlights its latest
/// text when the previous job ends.
pub fn poll(w: &mut World) -> bool {
    let mut changed = false;
    if w.syntax.chan.is_none() {
        w.syntax.chan = Some(w.waker.handle(crate::svc::Svc::Syntax).channel());
    }
    let done: Vec<Done> = w.syntax.chan.as_ref().unwrap().1.try_iter().collect();
    for d in done {
        let Some(job) = w.syntax.jobs.get_mut(&d.hl) else {
            continue;
        };
        if job.0.as_ref() == Some(&d.key) {
            job.0 = None;
        }
        job.1 = Some(d.key.clone());
        let te = d.key.0;
        if w.dead.contains(&te) || te >= w.items.len() {
            continue;
        }
        w.syntax.results.insert(
            te,
            Arc::new(Highlight {
                text: d.key.1,
                spans: d.spans,
            }),
        );
        w.text_restyled(te);
        changed = true;
    }
    let hls = w.of_type("SyntaxHighlighter");
    w.syntax.jobs.retain(|h, _| hls.contains(h));
    let live: Vec<usize> = hls
        .iter()
        .filter_map(|&h| match w.read(h, "textEdit") {
            Some(Value::Item(t)) => Some(t),
            _ => None,
        })
        .collect();
    w.syntax.results.retain(|t, _| live.contains(t));
    for h in hls {
        w.mirror(h, crate::svc::Svc::Syntax);
        let Some(Value::Item(te)) = w.read(h, "textEdit") else {
            continue;
        };
        w.mirror(te, crate::svc::Svc::Syntax);
        let text = str_of(w, te, "text");
        let def = str_of(w, h, "definition");
        let theme = str_of(w, h, "theme");
        let key = (te, text, def, theme);
        let job = w.syntax.jobs.entry(h).or_default();
        if job.0.is_some() || job.1.as_ref() == Some(&key) {
            continue;
        }
        job.0 = Some(key.clone());
        let tx = w.syntax.chan.as_ref().unwrap().0.clone();
        workpool::run(move || {
            let spans = highlight(&key.1, &key.2, &key.3);
            let _ = tx.send(Done { hl: h, key, spans });
        });
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The output of KSyntaxHighlighting's own `testhighlighter` for a text.
    fn reference(text: &str, def: &kate::Definition) -> String {
        let mut out = String::new();
        let mut state = kate::State::default();
        let mut cache = kate::Cache::default();
        for line in text.lines() {
            for (from, to, format) in def.line(line, &mut state, &mut cache) {
                let name = format.map_or("", |f| def.formats[f].name.as_str());
                let name = if name.is_empty() { "dsNormal" } else { name };
                out += &format!("<{name}>{}</{name}>", &line[from..to]);
            }
            out += "<br/>\n";
        }
        out
    }

    #[test]
    fn every_definition_loads() {
        for d in DEFINITIONS {
            let raw = kate::parsed(unpacked(d.2)).unwrap_or_else(|| panic!("{} is not read", d.0));
            assert_eq!(raw.name(), d.0);
            assert_eq!(raw.alternative_names(), d.1, "{}", d.0);
            let def = definition(d.0).unwrap_or_else(|| panic!("{} does not load", d.0));
            assert_eq!(def.broken_patterns(), Vec::<String>::new(), "{}", d.0);
        }
    }

    /// The definitions `xml` takes rules from (`context="...##Name"`) that are carried.
    fn carried_inside(xml: &str) -> Vec<&str> {
        let mut found = std::collections::BTreeSet::new();
        for value in xml.split("context=\"").skip(1) {
            let value = &value[..value.find('"').unwrap()];
            for part in value.split('!') {
                if let Some((_, name)) = part.split_once("##") {
                    found.insert(name);
                }
            }
        }
        let carried = |n: &&str| DEFINITIONS.iter().any(|d| d.0 == *n);
        found.into_iter().filter(carried).collect()
    }

    /// docs/quickshell-differences.md, "Code inside Markdown and LaTeX code blocks": the two
    /// lists of that section, and what is drawn for a language in them and for one outside.
    #[test]
    fn nested_code_is_coloured_for_the_languages_of_the_set() {
        let text = |name: &str| unpacked(DEFINITIONS.iter().find(|d| d.0 == name).unwrap().2);
        assert_eq!(
            carried_inside(text("Markdown")),
            [
                "Bash",
                "C",
                "C#",
                "C++",
                "CMake",
                "CSS",
                "Comments",
                "Doxygen",
                "Go",
                "HTML",
                "JSON",
                "Java",
                "JavaScript",
                "JavaScript React (JSX)",
                "Mustache/Handlebars (HTML)",
                "Python",
                "QML",
                "Ruby",
                "Rust",
                "TOML",
                "TypeScript",
                "XML",
                "YAML",
                "reStructuredText",
            ]
        );
        assert_eq!(
            carried_inside(text("LaTeX")),
            [
                "Bash",
                "C",
                "C#",
                "C++",
                "CMake",
                "CSS",
                "Comments",
                "Diff",
                "Go",
                "HTML",
                "JSON",
                "Java",
                "JavaScript",
                "Kotlin",
                "LaTeX",
                "Lua",
                "Makefile",
                "Markdown",
                "Nix",
                "Python",
                "QML",
                "Ruby",
                "Rust",
                "SQL",
                "Swift",
                "TOML",
                "TypeScript",
                "XML",
                "YAML",
                "reStructuredText",
            ]
        );
        // The colours of the code between the fences of a Markdown block.
        let colours = |lang: &str, code: &str| {
            let text = format!("```{lang}\n{code}\n```\n");
            let from = text.find('\n').unwrap() + 1;
            let to = from + code.len();
            let mut all: Vec<_> = highlight(&text, "markdown", "Monokai")
                .iter()
                .filter(|s| s.start >= from && s.end <= to)
                .map(|s| s.color)
                .collect();
            all.sort();
            all.dedup();
            all.len()
        };
        assert!(colours("python", "def f(x): return 'a'  # b") > 2);
        assert_eq!(colours("perl", "my $n = 'a'; # b"), 1);
    }

    /// Development aid: `KSH_AUTOTESTS=<syntax-highlighting>/autotests` compares with the
    /// library's own expected output for every input file one of our definitions claims.
    #[test]
    #[ignore = "needs a checkout of KSyntaxHighlighting"]
    fn library_autotests() {
        let Ok(dir) = std::env::var("KSH_AUTOTESTS") else {
            return;
        };
        let only = std::env::var("KSH_ONLY").ok();
        let attr = |xml: &str, k: &str| -> String {
            let tag = &xml[xml.find("<language").unwrap()..];
            let tag = &tag[..tag.find('>').unwrap()];
            tag.find(&format!(" {k}=\""))
                .or_else(|| tag.find(&format!("\n{k}=\"")))
                .map(|i| {
                    let rest = &tag[i + k.len() + 3..];
                    rest[..rest.find('"').unwrap()].to_string()
                })
                .unwrap_or_default()
        };
        let mut names: Vec<_> = std::fs::read_dir(format!("{dir}/input"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| !n.ends_with(".syntax") && n != ".clang-format")
            .collect();
        names.sort();
        // Every description of the library, to choose the one it chooses for a file name.
        let all: Vec<String> = std::fs::read_dir(format!("{dir}/../data/syntax"))
            .unwrap()
            .flatten()
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .filter(|x| x.contains("<language"))
            .collect();
        let show: usize = std::env::var("KSH_SHOW")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let (mut same, mut differ) = (0, 0);
        for n in names {
            let over = std::fs::read_to_string(format!("{dir}/input/{n}.syntax")).ok();
            let def = match &over {
                Some(o) => find(o.trim()),
                None => all
                    .iter()
                    .filter(|d| {
                        attr(d, "extensions")
                            .split(';')
                            .any(|e| match e.strip_prefix('*') {
                                Some(suffix) => !suffix.is_empty() && n.ends_with(suffix),
                                None => !e.is_empty() && e == n,
                            })
                    })
                    .max_by_key(|d| attr(d, "priority").parse::<i32>().unwrap_or(0))
                    .and_then(|d| find(&attr(d, "name"))),
            };
            let Some(def) = def else {
                continue;
            };
            if only.as_ref().is_some_and(|o| !n.contains(o.as_str())) {
                continue;
            }
            let text = String::from_utf8_lossy(&std::fs::read(format!("{dir}/input/{n}")).unwrap())
                .to_string();
            let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
            let want =
                std::fs::read_to_string(format!("{dir}/reference/{n}.ref")).unwrap_or_default();
            let got = reference(text, &definition(def).unwrap());
            if got == want {
                same += 1;
                continue;
            }
            differ += 1;
            let lines = want
                .lines()
                .zip(got.lines())
                .filter(|(a, b)| a != b)
                .count();
            println!(
                "DIFF {n} [{def}]: {lines} of {} lines",
                want.lines().count()
            );
            for (i, (a, b)) in want
                .lines()
                .zip(got.lines())
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .take(show)
            {
                println!("  line {}\n  want {a}\n  got  {b}", i + 1);
            }
        }
        println!("same {same}, differ {differ}");
    }
}
