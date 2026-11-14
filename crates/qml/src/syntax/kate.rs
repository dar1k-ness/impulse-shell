//! KSyntaxHighlighting's definitions (Kate's XML) and its line highlighter, 6.31: what
//! `definition.cpp`, `context.cpp`, `rule.cpp` and `abstracthighlighter.cpp` do, in their order.
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use super::pcre;

/// The characters that end a word.
#[derive(Clone)]
struct Delims {
    ascii: [bool; 128],
    other: Vec<char>,
}

impl Delims {
    fn new() -> Self {
        let mut d = Delims {
            ascii: [false; 128],
            other: vec![],
        };
        d.set("\t !%&()*+,-./:;<=>?[\\]^{|}~", true);
        d
    }
    fn set(&mut self, s: &str, on: bool) {
        for c in s.chars() {
            if (c as u32) < 128 {
                self.ascii[c as usize] = on;
            } else if on {
                self.other.push(c);
            } else {
                self.other.retain(|&o| o != c);
            }
        }
    }
    fn with(&self, add: &str, weak: &str) -> Delims {
        let mut d = self.clone();
        d.set(add, true);
        d.set(weak, false);
        d
    }
    fn has(&self, c: char) -> bool {
        if (c as u32) < 128 {
            self.ascii[c as usize]
        } else {
            self.other.contains(&c)
        }
    }
}

/// An `itemData`: a default style and what the definition sets over it.
#[derive(Clone, Debug, Default)]
pub struct Format {
    pub definition: String,
    pub name: String,
    pub style: String,
    pub color: Option<[u8; 4]>,
    pub background: Option<[u8; 4]>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
}

#[derive(Default)]
struct RawRule {
    tag: String,
    attrs: HashMap<String, String>,
}

struct RawContext {
    attrs: HashMap<String, String>,
    rules: Vec<RawRule>,
}

/// One XML file as read.
pub struct Raw {
    name: String,
    names: Vec<String>,
    case_sensitive: bool,
    delims: Delims,
    lists: Vec<(String, Vec<String>, Vec<String>)>,
    contexts: Vec<RawContext>,
    formats: Vec<Format>,
}

fn truthy(s: Option<&String>) -> bool {
    s.is_some_and(|s| s == "1" || s.eq_ignore_ascii_case("true"))
}

/// `QColor(name)` for the forms the definitions use: `#rgb`, `#rrggbb`, `#aarrggbb`.
pub fn color(s: &str) -> Option<[u8; 4]> {
    let h = s.strip_prefix('#')?;
    let v = u32::from_str_radix(h, 16).ok()?;
    let b = |n: u32| (v >> n) as u8;
    match h.len() {
        3 => Some([b(8) & 15, b(4) & 15, b(0) & 15, 255])
            .map(|c| [c[0] * 17, c[1] * 17, c[2] * 17, 255]),
        6 => Some([b(16), b(8), b(0), 255]),
        8 => Some([b(16), b(8), b(0), b(24)]),
        _ => None,
    }
}

fn elements<'a, 'i>(n: roxmltree::Node<'a, 'i>) -> Vec<roxmltree::Node<'a, 'i>> {
    n.children().filter(|c| c.is_element()).collect()
}

/// The text of an entity or of a character reference: `&#9;`, `&#x9;`, `&lt;` and the
/// document's own entities, which may name each other.
fn reference(name: &str, entities: &HashMap<String, String>, depth: usize) -> Option<String> {
    if let Some(n) = name.strip_prefix('#') {
        let code = match n.strip_prefix('x') {
            Some(h) => u32::from_str_radix(h, 16).ok()?,
            None => n.parse().ok()?,
        };
        return char::from_u32(code).map(String::from);
    }
    match name {
        "lt" => Some("<".into()),
        "gt" => Some(">".into()),
        "amp" => Some("&".into()),
        "quot" => Some("\"".into()),
        "apos" => Some("'".into()),
        _ if depth > 32 => None,
        _ => Some(references(entities.get(name)?, entities, depth + 1, false)),
    }
}

/// `text` with its references replaced. `own` replaces only the document's own entities
/// and writes their text as character references, which is how `QXmlStreamReader` gives an
/// attribute the tab of `&tab;` where a tab typed in the attribute is a space.
fn references(text: &str, entities: &HashMap<String, String>, depth: usize, own: bool) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out += &rest[..at];
        rest = &rest[at..];
        let name = rest.find(';').map(|end| &rest[1..end]);
        let known = name.filter(|n| !own || entities.contains_key(*n));
        match known.and_then(|n| reference(n, entities, depth)) {
            Some(value) if own => {
                for c in value.chars() {
                    out += &format!("&#{};", c as u32);
                }
                rest = &rest[name.unwrap().len() + 2..];
            }
            Some(value) => {
                out += &value;
                rest = &rest[name.unwrap().len() + 2..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out + rest
}

/// The document without its DOCTYPE and with the entities that declared replaced.
fn without_entities(xml: &str) -> String {
    let Some(start) = xml.find("<!DOCTYPE") else {
        return xml.to_string();
    };
    let subset = xml[start..].find('[').map(|i| start + i);
    let first_tag = xml[start..].find('>').map_or(xml.len(), |i| start + i);
    let (decls, end) = match subset {
        Some(open) if open < first_tag => {
            let close = xml[open..].find("]>").map_or(xml.len(), |i| open + i);
            (&xml[open + 1..close], (close + 2).min(xml.len()))
        }
        _ => ("", (first_tag + 1).min(xml.len())),
    };
    let mut entities = HashMap::new();
    let mut rest = decls;
    while let Some(at) = rest.find("<!ENTITY") {
        rest = &rest[at + 8..];
        let name_end = rest.trim_start().find(char::is_whitespace).unwrap_or(0);
        let name = rest.trim_start()[..name_end].to_string();
        let Some(open) = rest.find(['"', '\'']) else {
            break;
        };
        let quote = rest[open..].chars().next().unwrap();
        let Some(len) = rest[open + 1..].find(quote) else {
            break;
        };
        let value = &rest[open + 1..open + 1 + len];
        // Character references are replaced where the entity is declared, the rest where it
        // is used.
        let mut text = String::new();
        let mut v = value;
        while let Some(at) = v.find("&#") {
            text += &v[..at];
            v = &v[at..];
            let end = v.find(';').unwrap_or(v.len() - 1);
            match reference(&v[1..end], &entities, 0) {
                Some(c) if c == "&" => text += "&#38;",
                Some(c) => text += &c,
                None => text += &v[..=end],
            }
            v = &v[end + 1..];
        }
        entities.entry(name).or_insert(text + v);
        rest = &rest[open + 1 + len..];
    }
    format!(
        "{}{}",
        &xml[..start],
        references(&xml[end..], &entities, 0, true)
    )
}

fn parse(xml: &str) -> Option<Raw> {
    let text = without_entities(xml);
    let doc = roxmltree::Document::parse(&text).ok()?;
    let lang = doc.root_element();
    let attrs = |n: roxmltree::Node| -> HashMap<String, String> {
        n.attributes()
            .map(|a| (a.name().to_string(), a.value().to_string()))
            .collect()
    };
    let la = attrs(lang);
    let mut raw = Raw {
        name: la.get("name")?.clone(),
        names: la
            .get("alternativeNames")
            .map(|s| {
                s.split(';')
                    .filter(|n| !n.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        case_sensitive: true,
        delims: Delims::new(),
        lists: vec![],
        contexts: vec![],
        formats: vec![],
    };
    for top in elements(lang) {
        match top.tag_name().name() {
            "highlighting" => {
                for part in elements(top) {
                    match part.tag_name().name() {
                        "list" => {
                            let (mut items, mut includes) = (vec![], vec![]);
                            for e in elements(part) {
                                let text = e.text().unwrap_or("").trim().to_string();
                                match e.tag_name().name() {
                                    "item" => items.push(text),
                                    "include" => includes.push(text),
                                    _ => {}
                                }
                            }
                            let name = part.attribute("name").unwrap_or("").to_string();
                            raw.lists.push((name, items, includes));
                        }
                        "contexts" => {
                            for c in elements(part) {
                                if c.tag_name().name() != "context" {
                                    continue;
                                }
                                raw.contexts.push(RawContext {
                                    attrs: attrs(c),
                                    rules: elements(c)
                                        .into_iter()
                                        .map(|r| RawRule {
                                            tag: r.tag_name().name().to_string(),
                                            attrs: attrs(r),
                                        })
                                        .collect(),
                                });
                            }
                        }
                        "itemDatas" => {
                            for i in elements(part) {
                                if i.tag_name().name() != "itemData" {
                                    continue;
                                }
                                let a = attrs(i);
                                let flag = |k: &str| {
                                    a.get(k).filter(|v| !v.is_empty()).map(|v| truthy(Some(v)))
                                };
                                raw.formats.push(Format {
                                    definition: raw.name.clone(),
                                    name: a.get("name").cloned().unwrap_or_default(),
                                    style: a
                                        .get("defStyleNum")
                                        .and_then(|s| s.strip_prefix("ds"))
                                        .unwrap_or("Normal")
                                        .to_string(),
                                    color: a.get("color").and_then(|c| color(c)),
                                    background: a.get("backgroundColor").and_then(|c| color(c)),
                                    bold: flag("bold"),
                                    italic: flag("italic"),
                                    underline: flag("underline"),
                                    strike: flag("strikeOut"),
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            "general" => {
                for g in elements(top) {
                    if g.tag_name().name() == "keywords" {
                        if let Some(c) = g.attribute("casesensitive") {
                            raw.case_sensitive = truthy(Some(&c.to_string()));
                        }
                        raw.delims
                            .set(g.attribute("additionalDeliminator").unwrap_or(""), true);
                        raw.delims
                            .set(g.attribute("weakDeliminator").unwrap_or(""), false);
                    }
                }
            }
            _ => {}
        }
    }
    Some(raw)
}

/// Where a rule or a line end sends the context stack.
#[derive(Clone, Default)]
struct Switch {
    pop: usize,
    push: Vec<usize>,
    stay: bool,
}

struct Words {
    exact: HashSet<String>,
    folded: HashSet<String>,
}

enum Kind {
    AnyChar(Vec<char>),
    DetectChar(char),
    DynamicChar(usize),
    Detect2Chars(char, char),
    DetectIdentifier,
    DetectSpaces,
    Float(Delims),
    Int(Delims),
    HlCHex(Delims),
    HlCOct(Delims),
    HlCStringChar,
    HlCChar,
    Keyword(Arc<Words>, bool, Delims),
    LineContinue(char),
    RangeDetect(char, char),
    RegExpr(OnceLock<Option<pcre::Regex>>, String, bool, bool),
    DynamicRegExpr(String, bool, bool),
    StringDetect(Vec<char>, bool, bool),
    WordDetect(Vec<char>, bool, Delims),
    Include(String, bool),
}

struct Rule {
    kind: Kind,
    format: Option<usize>,
    switch: Switch,
    first_non_space: bool,
    look_ahead: bool,
    column: i32,
    dynamic: bool,
    skips: bool,
}

#[derive(Default)]
struct Context {
    format: Option<usize>,
    line_end: Switch,
    line_empty: Switch,
    fallthrough: Switch,
    stop_empty: bool,
    rules: Vec<Arc<Rule>>,
    has_dynamic: bool,
    /// 0 unresolved, 1 resolving, 2 resolved.
    resolve: u8,
}

struct Loaded {
    raw: Arc<Raw>,
    lists: HashMap<String, (Vec<String>, Vec<String>)>,
    words: HashMap<String, Arc<Words>>,
    keywords_loaded: bool,
    formats: HashMap<String, usize>,
    base: usize,
    loaded: bool,
}

/// A definition with every definition it includes, ready to highlight.
pub struct Definition {
    pub name: String,
    pub formats: Vec<Format>,
    contexts: Vec<Context>,
}

/// The XML of the definition `name` names (its name or an alternative one, any case).
pub type Source = dyn Fn(&str) -> Option<Arc<RawDefinition>>;

/// A parsed XML file.
pub type RawDefinition = Raw;

impl Raw {
    pub fn parse(xml: &str) -> Option<Self> {
        parse(xml)
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn alternative_names(&self) -> &[String] {
        &self.names
    }
}

struct Loader<'a> {
    source: &'a Source,
    defs: Vec<Loaded>,
    by_name: HashMap<String, Option<usize>>,
    contexts: Vec<Context>,
    owner: Vec<usize>,
    formats: Vec<Format>,
}

impl Loader<'_> {
    fn def(&mut self, name: &str) -> Option<usize> {
        let key = name.to_lowercase();
        if let Some(d) = self.by_name.get(&key) {
            return *d;
        }
        let found = (self.source)(name).map(|raw| {
            if let Some(i) = self.defs.iter().position(|d| Arc::ptr_eq(&d.raw, &raw)) {
                return i;
            }
            self.defs.push(Loaded {
                lists: raw
                    .lists
                    .iter()
                    .map(|(n, items, inc)| (n.clone(), (items.clone(), inc.clone())))
                    .collect(),
                raw,
                words: HashMap::new(),
                keywords_loaded: false,
                formats: HashMap::new(),
                base: 0,
                loaded: false,
            });
            self.defs.len() - 1
        });
        self.by_name.insert(key, found);
        found
    }

    fn list_keywords(&mut self, d: usize, name: &str) {
        loop {
            let Some(include) = self.defs[d].lists.get_mut(name).and_then(|l| l.1.pop()) else {
                return;
            };
            let found = match include.split_once("##") {
                Some((list, def)) => self.def(def).map(|o| {
                    self.keywords(o);
                    (o, list.to_string())
                }),
                None => Some((d, include.clone())),
            };
            let Some((o, list)) = found.filter(|(o, l)| self.defs[*o].lists.contains_key(l)) else {
                continue;
            };
            if (o, list.as_str()) != (d, name) {
                self.list_keywords(o, &list);
            }
            let more = self.defs[o].lists[&list].0.clone();
            self.defs[d].lists.get_mut(name).unwrap().0.extend(more);
        }
    }

    fn keywords(&mut self, d: usize) {
        if self.defs[d].keywords_loaded {
            return;
        }
        self.defs[d].keywords_loaded = true;
        let names: Vec<String> = self.defs[d].raw.lists.iter().map(|l| l.0.clone()).collect();
        for n in names {
            self.list_keywords(d, &n);
        }
    }

    fn words(&mut self, d: usize, list: &str) -> Option<Arc<Words>> {
        if let Some(w) = self.defs[d].words.get(list) {
            return Some(w.clone());
        }
        let items = &self.defs[d].lists.get(list)?.0;
        let w = Arc::new(Words {
            exact: items.iter().cloned().collect(),
            folded: items.iter().map(|i| i.to_lowercase()).collect(),
        });
        self.defs[d].words.insert(list.to_string(), w.clone());
        Some(w)
    }

    /// `DefinitionData::resolveIncludedContext`.
    fn context(&mut self, d: usize, def: &str, name: &str) -> Option<usize> {
        let o = if def.is_empty() {
            d
        } else {
            let o = self.def(def)?;
            if o != d {
                self.load(o);
            }
            o
        };
        let l = &self.defs[o];
        if !def.is_empty() && name.is_empty() {
            return (!l.raw.contexts.is_empty() && l.loaded).then_some(l.base);
        }
        if !l.loaded {
            return None;
        }
        let at = l
            .raw
            .contexts
            .iter()
            .position(|c| c.attrs.get("name").is_some_and(|n| n == name))?;
        Some(l.base + at)
    }

    /// `ContextSwitch::resolve`.
    fn switch(&mut self, d: usize, text: Option<&String>) -> Switch {
        let mut s = Switch {
            stay: true,
            ..Default::default()
        };
        let mut rest = text.map_or("", |t| t.as_str());
        if rest.is_empty() || rest == "#stay" {
            return s;
        }
        while let Some(r) = rest.strip_prefix("#pop") {
            s.pop += 1;
            if let Some(after) = r.strip_prefix('!') {
                rest = after;
                break;
            }
            rest = r;
        }
        s.stay = s.pop == 0;
        if rest.is_empty() {
            return s;
        }
        for part in rest.split('!') {
            let (name, def) = part.split_once("##").unwrap_or((part, ""));
            if let Some(c) = self.context(d, def, name) {
                s.push.push(c);
            }
        }
        s.stay = s.push.is_empty();
        s
    }

    fn rule(&mut self, d: usize, r: &RawRule) -> Option<Rule> {
        let a = &r.attrs;
        let get = |k: &str| a.get(k).map_or("", |s| s.as_str());
        let one = |k: &str| {
            let mut c = get(k).chars();
            c.next().filter(|_| c.next().is_none())
        };
        let insensitive = truthy(a.get("insensitive"));
        let dynamic = truthy(a.get("dynamic"));
        let delims = self.defs[d]
            .raw
            .delims
            .with(get("additionalDeliminator"), get("weakDeliminator"));
        let text = get("String");
        let kind = match r.tag.as_str() {
            "DetectChar" => match (one("char")?, dynamic) {
                (c, true) => Kind::DynamicChar(c.to_digit(10).unwrap_or(0) as usize),
                (c, false) => Kind::DetectChar(c),
            },
            "RegExpr" if text.is_empty() => return None,
            "RegExpr" if dynamic => {
                Kind::DynamicRegExpr(text.to_string(), insensitive, truthy(a.get("minimal")))
            }
            "RegExpr" => Kind::RegExpr(
                OnceLock::new(),
                text.to_string(),
                insensitive,
                truthy(a.get("minimal")),
            ),
            "IncludeRules" if get("context").is_empty() => return None,
            "IncludeRules" => {
                Kind::Include(get("context").to_string(), truthy(a.get("includeAttrib")))
            }
            "Detect2Chars" => Kind::Detect2Chars(one("char")?, one("char1")?),
            "keyword" if text.is_empty() => return None,
            "keyword" => {
                let words = self.words(d, text).filter(|w| !w.exact.is_empty())?;
                let sensitive = match a.get("insensitive") {
                    Some(_) => !insensitive,
                    None => self.defs[d].raw.case_sensitive,
                };
                Kind::Keyword(words, sensitive, delims)
            }
            "DetectSpaces" => Kind::DetectSpaces,
            "StringDetect" if text.is_empty() => return None,
            "StringDetect" => {
                let c: Vec<char> = text.chars().collect();
                let same = |c: char| c.to_lowercase().eq(c.to_uppercase());
                match c[..] {
                    [c] if !dynamic && (!insensitive || same(c)) => Kind::DetectChar(c),
                    [c] if !dynamic => {
                        Kind::AnyChar(c.to_lowercase().chain(c.to_uppercase()).collect())
                    }
                    [a, b] if !dynamic && !insensitive => Kind::Detect2Chars(a, b),
                    _ => Kind::StringDetect(c, insensitive, dynamic),
                }
            }
            "WordDetect" if text.is_empty() => return None,
            "WordDetect" => Kind::WordDetect(text.chars().collect(), insensitive, delims),
            "AnyChar" if text.is_empty() => return None,
            "AnyChar" => match text.chars().collect::<Vec<_>>()[..] {
                [c] => Kind::DetectChar(c),
                ref c => Kind::AnyChar(c.to_vec()),
            },
            "DetectIdentifier" => Kind::DetectIdentifier,
            "LineContinue" => Kind::LineContinue(get("char").chars().next().unwrap_or('\\')),
            "Int" => Kind::Int(delims),
            "Float" => Kind::Float(delims),
            "HlCStringChar" => Kind::HlCStringChar,
            "RangeDetect" => Kind::RangeDetect(one("char")?, one("char1")?),
            "HlCHex" => Kind::HlCHex(delims),
            "HlCChar" => Kind::HlCChar,
            "HlCOct" => Kind::HlCOct(delims),
            _ => return None,
        };
        let mut rule = Rule {
            dynamic: matches!(
                kind,
                Kind::DynamicChar(_) | Kind::DynamicRegExpr(..) | Kind::StringDetect(_, _, true)
            ),
            skips: matches!(
                kind,
                Kind::Keyword(..) | Kind::RegExpr(..) | Kind::DynamicRegExpr(..)
            ),
            kind,
            format: None,
            switch: Switch {
                stay: true,
                ..Default::default()
            },
            first_non_space: false,
            look_ahead: false,
            column: -1,
        };
        if matches!(rule.kind, Kind::Include(..)) {
            return Some(rule);
        }
        rule.first_non_space = truthy(a.get("firstNonSpace"));
        rule.look_ahead = truthy(a.get("lookAhead"));
        if !rule.look_ahead {
            rule.format = self.defs[d].formats.get(get("attribute")).copied();
        }
        rule.column = get("column").parse().unwrap_or(-1);
        rule.switch = self.switch(d, a.get("context"));
        (!(rule.look_ahead && rule.switch.stay)).then_some(rule)
    }

    /// `DefinitionData::load` with `resolveContexts`.
    fn load(&mut self, d: usize) {
        if self.defs[d].loaded {
            return;
        }
        self.keywords(d);
        let raw = self.defs[d].raw.clone();
        for f in &raw.formats {
            self.defs[d]
                .formats
                .insert(f.name.clone(), self.formats.len());
            self.formats.push(f.clone());
        }
        self.defs[d].base = self.contexts.len();
        self.defs[d].loaded = true;
        let base = self.contexts.len();
        for c in &raw.contexts {
            let format = c
                .attrs
                .get("attribute")
                .and_then(|a| self.defs[d].formats.get(a))
                .copied();
            self.contexts.push(Context {
                format,
                ..Default::default()
            });
            self.owner.push(d);
        }
        for (i, c) in raw.contexts.iter().enumerate() {
            let line_end = self.switch(d, c.attrs.get("lineEndContext"));
            let mut line_empty = self.switch(d, c.attrs.get("lineEmptyContext"));
            let fallthrough = self.switch(d, c.attrs.get("fallthroughContext"));
            if line_empty.stay {
                line_empty = line_end.clone();
            }
            let ctx = &mut self.contexts[base + i];
            (ctx.line_end, ctx.line_empty, ctx.fallthrough) = (line_end, line_empty, fallthrough);
            ctx.stop_empty = truthy(c.attrs.get("stopEmptyLineContextSwitchLoop"));
            for r in &c.rules {
                if let Some(rule) = self.rule(d, r) {
                    self.contexts[base + i].rules.push(Arc::new(rule));
                }
            }
        }
        for i in 0..raw.contexts.len() {
            self.includes(base + i);
        }
    }

    /// `Context::resolveIncludes`.
    fn includes(&mut self, c: usize) {
        if self.contexts[c].resolve != 0 {
            return;
        }
        self.contexts[c].resolve = 1;
        let d = self.owner[c];
        let mut k = 0;
        while k < self.contexts[c].rules.len() {
            let rule = self.contexts[c].rules[k].clone();
            let Kind::Include(target, attrib) = &rule.kind else {
                self.contexts[c].has_dynamic |= rule.dynamic;
                k += 1;
                continue;
            };
            let found = match target.split_once("##") {
                Some((name, def)) => self.context(d, def, name),
                None => self.context(d, "", target),
            };
            let Some(t) = found.filter(|&t| t != c) else {
                k += 1;
                continue;
            };
            if self.contexts[t].resolve != 2 {
                self.includes(t);
            }
            self.contexts[c].has_dynamic |= self.contexts[t].has_dynamic;
            if *attrib {
                self.contexts[c].format = self.contexts[t].format;
            }
            let rules = self.contexts[t].rules.clone();
            let n = rules.len();
            self.contexts[c].rules.splice(k..k + 1, rules);
            k += n;
        }
        self.contexts[c].resolve = 2;
    }
}

impl Definition {
    /// The definition `name` names with all it includes, as `source` gives them.
    pub fn load(name: &str, source: &Source) -> Option<Definition> {
        let mut l = Loader {
            source,
            defs: vec![],
            by_name: HashMap::new(),
            contexts: vec![],
            owner: vec![],
            formats: vec![],
        };
        let d = l.def(name)?;
        l.load(d);
        if l.defs[d].raw.contexts.is_empty() {
            return None;
        }
        Some(Definition {
            name: l.defs[d].raw.name.clone(),
            formats: l.formats,
            contexts: l.contexts,
        })
    }
}

impl Definition {
    /// The patterns of `RegExpr` rules that do not compile.
    pub fn broken_patterns(&self) -> Vec<String> {
        let mut out = vec![];
        for rule in self.contexts.iter().flat_map(|c| &c.rules) {
            if let Kind::RegExpr(re, pattern, insensitive, minimal) = &rule.kind {
                let ok = re
                    .get_or_init(|| compile(pattern, *insensitive, *minimal))
                    .is_some();
                if !ok && !out.contains(pattern) {
                    out.push(pattern.clone());
                }
            }
        }
        out
    }
}

/// The context stack a line ends with: (context, captures of the rule that pushed it).
#[derive(Clone, Default)]
pub struct State(Vec<(usize, Arc<Vec<String>>)>);

struct Line<'a> {
    text: &'a str,
    chars: Vec<char>,
    bytes: Vec<usize>,
}

impl Line<'_> {
    fn at(&self, byte: usize) -> usize {
        self.bytes.partition_point(|&b| b < byte)
    }
}

fn digit(c: char) -> bool {
    c.is_ascii_digit()
}
fn octal(c: char) -> bool {
    ('0'..='7').contains(&c)
}

fn escaped(t: &[char], o: usize) -> usize {
    if t[o] != '\\' || t.len() < o + 2 {
        return o;
    }
    match t[o + 1] {
        'a' | 'b' | 'e' | 'f' | 'n' | 'r' | 't' | 'v' | '"' | '\'' | '?' | '\\' => o + 2,
        'x' => match (t.get(o + 2), t.get(o + 3)) {
            (Some(a), Some(b)) if a.is_ascii_hexdigit() && b.is_ascii_hexdigit() => o + 4,
            (Some(a), _) if a.is_ascii_hexdigit() => o + 3,
            _ => o,
        },
        '0'..='7' => match (t.get(o + 2), t.get(o + 3)) {
            (Some(&a), Some(&b)) if octal(a) && octal(b) => o + 4,
            (Some(&a), _) if octal(a) => o + 3,
            _ => o + 2,
        },
        _ => o,
    }
}

fn same(a: &[char], b: &[char], insensitive: bool) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x == y || (insensitive && x.to_lowercase().eq(y.to_lowercase())))
}

fn expand(pattern: &str, captures: &[String], quote: bool) -> String {
    let mut s = pattern.to_string();
    for i in (1..=captures.len()).rev() {
        let c = &captures[i - 1];
        s = s.replace(
            &format!("%{i}"),
            &if quote { pcre::escape(c) } else { c.clone() },
        );
    }
    s
}

fn compile(pattern: &str, insensitive: bool, minimal: bool) -> Option<pcre::Regex> {
    pcre::Regex::new(pattern, insensitive, minimal)
}

/// A rule's answer: the offset after the match (the offset itself for none), the offset up to
/// which the rule cannot match (negative: nowhere in the line), what its groups captured.
type Matched = (usize, i64, Vec<String>);

fn regex(re: Option<&pcre::Regex>, l: &Line, o: usize) -> Matched {
    let Some((start, end, caps)) = re.and_then(|re| re.find(l.text, l.bytes[o])) else {
        return (o, -1, vec![]);
    };
    if start != l.bytes[o] {
        return (o, l.at(start) as i64, vec![]);
    }
    (l.at(end), 0, caps)
}

impl Rule {
    fn matches(
        &self,
        l: &Line,
        o: usize,
        captures: &[String],
        cache: &mut HashMap<String, Option<pcre::Regex>>,
    ) -> Matched {
        let t = &l.chars[..];
        let n = t.len();
        let no = (o, 0, vec![]);
        let yes = |end: usize| (end, 0, vec![]);
        let after_word = |d: &Delims| o == 0 || d.has(t[o - 1]);
        match &self.kind {
            Kind::AnyChar(c) => yes(o + c.contains(&t[o]) as usize),
            Kind::DetectChar(c) => yes(o + (t[o] == *c) as usize),
            Kind::DynamicChar(i) => {
                let c = i
                    .checked_sub(1)
                    .and_then(|i| captures.get(i))
                    .and_then(|c| c.chars().next());
                yes(o + (c == Some(t[o])) as usize)
            }
            Kind::Detect2Chars(a, b) => {
                yes(o + 2 * (n - o >= 2 && t[o] == *a && t[o + 1] == *b) as usize)
            }
            Kind::DetectIdentifier => {
                if !t[o].is_alphabetic() && t[o] != '_' {
                    return no;
                }
                yes((o + 1..n)
                    .find(|&i| !t[i].is_alphanumeric() && t[i] != '_')
                    .unwrap_or(n))
            }
            Kind::DetectSpaces => yes((o..n).find(|&i| !t[i].is_whitespace()).unwrap_or(n)),
            Kind::Float(d) => {
                if !after_word(d) {
                    return no;
                }
                let digits = |from: usize| (from..n).find(|&i| !digit(t[i])).unwrap_or(n);
                let dot = digits(o);
                if dot >= n || t[dot] != '.' {
                    return no;
                }
                let end = digits(dot + 1);
                if end == o + 1 {
                    return no;
                }
                if end >= n || !matches!(t[end], 'e' | 'E') {
                    return yes(end);
                }
                let sign = end + 1 + (end + 1 < n && matches!(t[end + 1], '+' | '-')) as usize;
                let exp = digits(sign.min(n));
                yes(if exp > sign { exp } else { end })
            }
            Kind::Int(d) => {
                if !after_word(d) {
                    return no;
                }
                yes((o..n).find(|&i| !digit(t[i])).unwrap_or(n))
            }
            Kind::HlCHex(d) => {
                if !after_word(d)
                    || n < o + 3
                    || t[o] != '0'
                    || !matches!(t[o + 1], 'x' | 'X')
                    || !t[o + 2].is_ascii_hexdigit()
                {
                    return no;
                }
                yes((o + 3..n).find(|&i| !t[i].is_ascii_hexdigit()).unwrap_or(n))
            }
            Kind::HlCOct(d) => {
                if !after_word(d) || n < o + 2 || t[o] != '0' || !octal(t[o + 1]) {
                    return no;
                }
                yes((o + 2..n).find(|&i| !octal(t[i])).unwrap_or(n))
            }
            Kind::HlCStringChar => yes(escaped(t, o)),
            Kind::HlCChar => {
                if n < o + 3 || t[o] != '\'' || t[o + 1] == '\'' {
                    return no;
                }
                let mut e = escaped(t, o + 1);
                if e == o + 1 {
                    if t[e] == '\\' {
                        return no;
                    }
                    e += 1;
                }
                yes(if e < n && t[e] == '\'' { e + 1 } else { o })
            }
            Kind::Keyword(words, sensitive, d) => {
                let end = (o..n).find(|&i| d.has(t[i])).unwrap_or(n);
                if end == o {
                    return no;
                }
                let word: String = t[o..end].iter().collect();
                let known = if *sensitive {
                    words.exact.contains(&word)
                } else {
                    words.folded.contains(&word.to_lowercase())
                };
                if known {
                    yes(end)
                } else {
                    (o, end as i64, vec![])
                }
            }
            Kind::LineContinue(c) => yes(o + (o == n - 1 && t[o] == *c) as usize),
            Kind::RangeDetect(a, b) => {
                if n - o < 2 || t[o] != *a {
                    return no;
                }
                yes((o + 1..n).find(|&i| t[i] == *b).map_or(o, |i| i + 1))
            }
            Kind::RegExpr(re, pattern, insensitive, minimal) => regex(
                re.get_or_init(|| compile(pattern, *insensitive, *minimal))
                    .as_ref(),
                l,
                o,
            ),
            Kind::DynamicRegExpr(pattern, insensitive, minimal) => {
                let p = expand(pattern, captures, true);
                let re = cache
                    .entry(p)
                    .or_insert_with_key(|p| compile(p, *insensitive, *minimal));
                regex(re.as_ref(), l, o)
            }
            Kind::StringDetect(s, insensitive, dynamic) => {
                let expanded: Vec<char>;
                let s = if *dynamic {
                    expanded = expand(&s.iter().collect::<String>(), captures, false)
                        .chars()
                        .collect();
                    &expanded
                } else {
                    s
                };
                yes(o + s.len()
                    * (o + s.len() <= n && same(&t[o..o + s.len()], s, *insensitive)) as usize)
            }
            Kind::WordDetect(w, insensitive, d) => {
                let e = o + w.len();
                if n < e
                    || (o > 0 && !d.has(t[o - 1]) && !d.has(t[o]))
                    || !same(&t[o..e], w, *insensitive)
                {
                    return no;
                }
                yes(if n == e || d.has(t[e]) || d.has(t[e - 1]) {
                    e
                } else {
                    o
                })
            }
            Kind::Include(..) => no,
        }
    }
}

/// `AbstractHighlighterPrivate::switchContext`.
fn switch(stack: &mut Vec<(usize, Arc<Vec<String>>)>, s: &Switch, captures: Vec<String>) -> bool {
    if s.stay {
        return true;
    }
    let survived = stack.len() > s.pop;
    stack.truncate(stack.len().saturating_sub(s.pop).max(1));
    if s.push.is_empty() {
        return survived;
    }
    let captures = Arc::new(captures);
    stack.extend(s.push.iter().map(|&c| (c, captures.clone())));
    true
}

/// Dynamic patterns compiled so far, by the pattern with the captures in it.
#[derive(Default)]
pub struct Cache(HashMap<String, Option<pcre::Regex>>);

impl Definition {
    /// `AbstractHighlighter::highlightLine`: the runs of one line without its line end, as
    /// (start, end, format) in bytes of the line; `None` is the format of plain text. An
    /// empty line gives one empty run, as the library does.
    pub fn line(
        &self,
        text: &str,
        state: &mut State,
        cache: &mut Cache,
    ) -> Vec<(usize, usize, Option<usize>)> {
        let stack = &mut state.0;
        if stack.is_empty() {
            stack.push((0, Arc::new(vec![])));
        }
        let top = |stack: &Vec<(usize, Arc<Vec<String>>)>| &self.contexts[stack.last().unwrap().0];
        if text.is_empty() {
            let mut turns = 0;
            while !top(stack).line_empty.stay {
                let s = top(stack).line_empty.clone();
                if !switch(stack, &s, vec![]) || top(stack).stop_empty {
                    break;
                }
                turns += 1;
                if turns > 1024 {
                    break;
                }
            }
            return vec![(0, 0, top(stack).format)];
        }
        let mut l = Line {
            text,
            chars: vec![],
            bytes: vec![],
        };
        for (b, c) in text.char_indices() {
            l.bytes.push(b);
            l.chars.push(c);
        }
        l.bytes.push(text.len());
        let n = l.chars.len();
        let mut out = vec![];
        let (mut offset, mut begin, mut last, mut turns) = (0, 0, 0, 0);
        let mut continued = false;
        let mut skips: Vec<(*const Rule, i64)> = vec![];
        let mut skip_captures: Arc<Vec<String>> = Arc::new(vec![]);
        let mut format = top(stack).format;
        let mut first: Option<usize> = None;
        loop {
            if last == offset {
                turns += 1;
                if turns > 1024 {
                    break;
                }
            } else {
                last = offset;
                turns = 0;
            }
            let mut look_ahead = false;
            let mut new_offset = 0;
            let mut new_format = None;
            let rules = top(stack).rules.clone();
            for rule in &rules {
                if rule.column >= 0 && rule.column as usize != offset {
                    continue;
                }
                if rule.first_non_space {
                    let f = *first.get_or_insert_with(|| {
                        l.chars.iter().position(|c| !c.is_whitespace()).unwrap_or(n)
                    });
                    if offset > f {
                        continue;
                    }
                }
                let key = Arc::as_ptr(rule);
                let captures = stack.last().unwrap().1.clone();
                let mut skip = 0;
                if rule.skips {
                    if rule.dynamic && skip_captures != captures {
                        skips.clear();
                    } else {
                        skip = skips.iter().find(|s| s.0 == key).map_or(0, |s| s.1);
                        if skip < 0 || skip > offset as i64 {
                            continue;
                        }
                    }
                }
                let (end, new_skip, caps) = rule.matches(&l, offset, &captures, &mut cache.0);
                new_offset = end;
                if new_skip < 0 || new_skip > skip {
                    match skips.iter_mut().find(|s| s.0 == key) {
                        Some(s) => s.1 = new_skip,
                        None => skips.push((key, new_skip)),
                    }
                    if rule.dynamic {
                        skip_captures = captures.clone();
                    }
                }
                if new_offset <= offset {
                    continue;
                }
                switch(stack, &rule.switch, caps);
                if rule.look_ahead {
                    look_ahead = true;
                    break;
                }
                new_format = rule.format.or(top(stack).format);
                continued = new_offset == n && matches!(rule.kind, Kind::LineContinue(_));
                break;
            }
            if look_ahead {
                continue;
            }
            if new_offset <= offset {
                if !top(stack).fallthrough.stay {
                    let s = top(stack).fallthrough.clone();
                    switch(stack, &s, vec![]);
                    continue;
                }
                new_offset = offset + 1;
                new_format = top(stack).format;
            }
            if new_format != format {
                if offset > 0 {
                    out.push((l.bytes[begin], l.bytes[offset], format));
                }
                begin = offset;
                format = new_format;
            }
            offset = new_offset;
            if offset >= n {
                break;
            }
        }
        if begin < offset {
            out.push((l.bytes[begin], text.len(), format));
        }
        let mut turns = 0;
        while !top(stack).line_end.stay && !continued {
            let s = top(stack).line_end.clone();
            if !switch(stack, &s, vec![]) {
                break;
            }
            turns += 1;
            if turns > 1024 {
                break;
            }
        }
        out
    }
}

/// Parsed files by the address of their text.
pub fn parsed(xml: &'static str) -> Option<Arc<RawDefinition>> {
    static CACHE: OnceLock<Mutex<HashMap<usize, Option<Arc<RawDefinition>>>>> = OnceLock::new();
    let mut c = CACHE.get_or_init(Default::default).lock().unwrap();
    c.entry(xml.as_ptr() as usize)
        .or_insert_with(|| RawDefinition::parse(xml).map(Arc::new))
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `QXmlStreamReader` of Qt 6.12 gives for the same document.
    #[test]
    fn entities_and_attribute_white_space_as_qt_reads_them() {
        let doc = "<?xml version=\"1.0\"?>\n<!DOCTYPE language [\n  <!ENTITY tab \"&#9;\">\n  \
                   <!ENTITY lt2 \"&lt;>|&amp;;()\">\n  <!ENTITY w \" &tab;&lt2;\">\n  \
                   <!ENTITY q \"a&quot;b'c\">\n]>\n<language a=\"x&tab;y\" b=\"&w;\" \
                   c=\"lit\ttab\" d=\"&#9;\" e=\"&q;\" f=\"nl\nx\" g=\"&#10;\"/>";
        let text = without_entities(doc);
        let parsed = roxmltree::Document::parse(&text).unwrap();
        let root = parsed.root_element();
        for (name, value) in [
            ("a", "x\ty"),
            ("b", " \t<>|&;()"),
            ("c", "lit tab"),
            ("d", "\t"),
            ("e", "a\"b'c"),
            ("f", "nl x"),
            ("g", "\n"),
        ] {
            assert_eq!(root.attribute(name), Some(value), "{name}");
        }
    }
}
