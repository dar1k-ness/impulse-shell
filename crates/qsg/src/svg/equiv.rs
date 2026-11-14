//! roxmltree/svgtypes parsers against the pre-switch hand-written ones (`super::legacy`):
//! what they read from the original's SVG assets and system icon themes. The pixels are compared with
//! Qt itself (`tests/svg_stroke_qt.rs`).
//!
//! `cargo test -p qsg --release svg::equiv` runs the QML assets plus a capped system sample;
//! `--features live` adds every SVG under /usr/share/icons (timing: `cargo bench -p qsg`).
use super::legacy;
use crate::canvas::El;
use std::path::{Path, PathBuf};

const SCHEME: &str = ".ColorScheme-Text { color:#ff0000; } .ColorScheme-Highlight { color:#00ff00; } \
                      .ColorScheme-Background { color:#0000ff; } .ColorScheme-NegativeText { color:#ffff00; }";

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut ents: Vec<_> = rd.flatten().collect();
    ents.sort_by_key(|e| e.file_name());
    for e in ents {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            walk(&p, out);
        } else if ft.is_file() && p.extension().is_some_and(|x| x == "svg") {
            out.push(p);
        }
    }
}

fn ri_assets() -> Vec<PathBuf> {
    let mut v = vec![];
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml"),
        &mut v,
    );
    v
}

/// Every `step`-th SVG of the common themes, at most `cap` (deterministic, sorted walk).
fn system_icons(cap: usize) -> Vec<PathBuf> {
    let mut all = vec![];
    for t in [
        "hicolor",
        "Adwaita",
        "breeze",
        "breeze-dark",
        "Humanity",
        "ubuntu-mono-dark",
    ] {
        walk(&Path::new("/usr/share/icons").join(t), &mut all);
    }
    let step = all.len().div_ceil(cap.max(1)).max(1);
    all.into_iter().step_by(step).take(cap).collect()
}

fn all_system_icons() -> Vec<PathBuf> {
    let mut v = vec![];
    walk(Path::new("/usr/share/icons"), &mut v);
    v
}

/// XML attribute-value normalization (the old parser kept raw whitespace).
fn xml_norm(v: &str) -> String {
    v.replace("\r\n", "\n")
        .chars()
        .map(|c| {
            if matches!(c, '\t' | '\n' | '\r') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

#[derive(Default)]
struct Report {
    files: usize,
    attrs: usize,
    /// (category, file, detail)
    diffs: Vec<(&'static str, String, String)>,
    /// Differences where the new parser follows QtSvg and the old one did not.
    intentional: Vec<(&'static str, String, String)>,
}

impl Report {
    fn diff(&mut self, cat: &'static str, f: &Path, d: String) {
        self.diffs.push((cat, f.display().to_string(), d));
    }

    fn summary(&self) -> String {
        let mut s = String::new();
        for (cat, f, d) in &self.intentional {
            s += &format!("  intentional ({cat}): {f} :: {d}\n");
        }
        let mut cats: Vec<&str> = self.diffs.iter().map(|d| d.0).collect();
        cats.sort();
        cats.dedup();
        s = format!(
            "{} files, {} parsed attribute values compared, {} differences, \
             {} intentional\n{s}",
            self.files,
            self.attrs,
            self.diffs.len(),
            self.intentional.len()
        );
        for c in cats {
            let ds: Vec<_> = self.diffs.iter().filter(|d| d.0 == c).collect();
            s += &format!("  {c}: {}\n", ds.len());
            for d in ds.iter().take(5) {
                s += &format!("    {} :: {}\n", d.1, d.2);
            }
        }
        s
    }
}

/// A line or a curve (`true`) with the point it starts from.
type Segment = (bool, [[f64; 2]; 4]);

/// What a path draws, without the lines and curves of no length. `super::parse_path` builds a
/// QPainterPath as Qt does, which keeps none of those, ends a closed subpath on its first
/// point and starts the next one with a MoveTo; the reference kept every element as written.
fn segments(p: &[El]) -> Vec<Segment> {
    let mut out = vec![];
    let mut cur = [0.0; 2];
    for e in p {
        match *e {
            El::Move(a) => cur = a,
            El::Line(a) => {
                if !super::fuzzy_pt(cur, a) {
                    out.push((false, [cur, cur, a, a]));
                    cur = a;
                }
            }
            El::Cubic(a, b, c) => {
                if ![a, b, c].iter().all(|p| super::fuzzy_pt(cur, *p)) {
                    out.push((true, [cur, a, b, c]));
                    cur = c;
                }
            }
        }
    }
    out
}

fn bits(v: &[f64]) -> Vec<u64> {
    v.iter().map(|x| x.to_bits()).collect()
}

const COLOR_KEYS: &[&str] = &[
    "fill",
    "stroke",
    "stop-color",
    "color",
    "flood-color",
    "lighting-color",
];
const NUM_KEYS: &[&str] = &[
    "x",
    "y",
    "width",
    "height",
    "rx",
    "ry",
    "cx",
    "cy",
    "r",
    "fx",
    "fy",
    "x1",
    "y1",
    "x2",
    "y2",
    "offset",
    "opacity",
    "fill-opacity",
    "stroke-opacity",
    "stop-opacity",
    "stroke-width",
];

/// Runs every replaced parser on one attribute value, old vs new.
fn cmp_value(r: &mut Report, f: &Path, k: &str, v: &str) {
    let d = |o: String, n: String| format!("{k}={v:?}: old {o} new {n}");
    match k {
        "d" => {
            r.attrs += 1;
            let (o, n) = (legacy::parse_path(v), super::parse_path(v));
            let (so, sn) = (segments(&o), segments(&n));
            let same = |a: &Segment, b: &Segment| {
                a.0 == b.0 && a.1.iter().zip(&b.1).all(|(p, q)| super::fuzzy_pt(*p, *q))
            };
            if so.len() != sn.len() || !so.iter().zip(&sn).all(|(a, b)| same(a, b)) {
                r.diff(
                    "path",
                    f,
                    d(
                        format!("{} segments", so.len()),
                        format!("{} segments", sn.len()),
                    ),
                );
            }
        }
        "transform" | "gradientTransform" | "patternTransform" => {
            r.attrs += 1;
            let (o, n) = (legacy::parse_transform(v), super::parse_transform(v));
            if bits(&o) != bits(&n) {
                r.diff("transform", f, d(format!("{o:?}"), format!("{n:?}")));
            }
        }
        "viewBox" | "points" => {
            r.attrs += 1;
            let (o, n) = (legacy::nums(v), super::nums(v));
            if bits(&o) != bits(&n) {
                r.diff("number list", f, d(format!("{o:?}"), format!("{n:?}")));
            }
        }
        _ if COLOR_KEYS.contains(&k) => {
            r.attrs += 1;
            let (o, n) = (legacy::parse_color(v), super::parse_color(v));
            let why = if is_rgb_percent(v) {
                "rgb() percentages"
            } else if o.is_none() && v.trim().bytes().all(|b| b.is_ascii_alphabetic()) {
                "SVG color keyword"
            } else {
                "color"
            };
            if o != n && why != "color" {
                r.intentional.push((
                    why,
                    f.display().to_string(),
                    d(format!("{o:?}"), format!("{n:?}")),
                ));
            } else if o != n {
                r.diff("color", f, d(format!("{o:?}"), format!("{n:?}")));
            }
            let (o, n) = (legacy::url_id(v), super::url_id(v));
            if o != n {
                r.diff("url()", f, d(format!("{o:?}"), format!("{n:?}")));
            }
        }
        _ if NUM_KEYS.contains(&k) => {
            r.attrs += 1;
            let (o, n) = (legacy::num_prefix(v), super::num_prefix(v));
            if o.to_bits() != n.to_bits() {
                r.diff("number", f, d(format!("{o:?}"), format!("{n:?}")));
            }
        }
        "mask" => {
            r.attrs += 1;
            let (o, n) = (legacy::url_id(v), super::url_id(v));
            if o != n {
                r.diff("url()", f, d(format!("{o:?}"), format!("{n:?}")));
            }
        }
        _ => {}
    }
}

fn cmp_tree(r: &mut Report, f: &Path, o: &legacy::Elem, n: &super::Elem) {
    // QSvgHandler goes by the local name, the reference kept the prefix.
    if o.name.rsplit(':').next() != Some(&n.name) {
        r.diff("tree", f, format!("element {} vs {}", o.name, n.name));
        return;
    }
    let oa: Vec<(String, String)> = o
        .attrs
        .iter()
        .filter(|(k, _)| k != "xmlns" && !k.starts_with("xmlns:"))
        .map(|(k, v)| (k.clone(), xml_norm(v)))
        .collect();
    if oa != n.attrs {
        r.diff(
            "tree",
            f,
            format!(
                "<{}> attrs\n      old {oa:?}\n      new {:?}",
                o.name, n.attrs
            ),
        );
    }
    for (k, v) in &n.attrs {
        cmp_value(r, f, k, v);
        if k == "style" {
            for (k, v) in super::decls(v) {
                cmp_value(r, f, &k, &v);
            }
        }
    }
    // The reference kept no character data.
    let kept: Vec<&super::Elem> = n.children.iter().filter(|c| c.name != "#text").collect();
    if o.children.len() != kept.len() {
        r.diff(
            "tree",
            f,
            format!(
                "<{}> {} vs {} children",
                o.name,
                o.children.len(),
                kept.len()
            ),
        );
        return;
    }
    for (a, b) in o.children.iter().zip(kept) {
        cmp_tree(r, f, a, b);
    }
}

/// `rgb(x%, y%, z%)`: QSvgHandler resolveColor scales percentages by 2.55 and truncates; the old
/// parser read them as 0 (black).
fn is_rgb_percent(v: &str) -> bool {
    v.trim_start().starts_with("rgb(") && v.contains('%')
}

/// `src` with every `rgb(..%..)` written as the integer `rgb()` the new parser resolves it to, so
/// the old code can render the QtSvg colors and the rest of the pipeline is still compared.
fn qt_rgb_percent(src: &str) -> String {
    let mut out = String::new();
    let mut rest = src;
    while let Some(a) = rest.find("rgb(") {
        out.push_str(&rest[..a]);
        let end = rest[a..].find(')').map_or(rest.len(), |e| a + e + 1);
        let v = &rest[a..end];
        match super::parse_color(v) {
            Some(Some([r, g, b, _])) if v.contains('%') => out += &format!("rgb({r},{g},{b})"),
            _ => out.push_str(v),
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// SVG color keywords the old parser did not know (it only had black/white/red/green/blue/gray),
/// written as `#rrggbb` where they stand as a whole attribute or style value.
fn qt_keywords(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_alphabetic() || (i > 0 && b[i - 1].is_ascii_alphanumeric()) {
            out.push(b[i] as char);
            i += 1;
            continue;
        }
        let mut j = i;
        while j < b.len() && b[j].is_ascii_alphabetic() {
            j += 1;
        }
        let w = &src[i..j];
        let before = src[..i].trim_end().bytes().last();
        let after = src[j..].trim_start().bytes().next();
        let value = matches!(before, Some(b'"' | b'\'' | b':'))
            && matches!(after, Some(b'"' | b'\'' | b';'));
        match (value, legacy::parse_color(w), super::parse_color(w)) {
            (true, None, Some(Some([r, g, b, _]))) => out += &format!("#{r:02x}{g:02x}{b:02x}"),
            _ => out.push_str(w),
        }
        i = j;
    }
    out
}

/// Internal DTD entities (`<!ENTITY st0 "fill:...">`, Adobe Illustrator exports) expanded as
/// QXmlStreamReader (QtSvg) and roxmltree do; the old parser left `&st0;` in attribute values.
fn expand_entities(src: &str) -> String {
    let Some(a) = src.find("<!DOCTYPE") else {
        return src.to_string();
    };
    let Some(e) = src[a..].find("]>").map(|e| a + e) else {
        return src.to_string();
    };
    let mut ents = vec![];
    let mut dtd = &src[a..e];
    while let Some(k) = dtd.find("<!ENTITY") {
        dtd = &dtd[k + 8..];
        let t = dtd.trim_start();
        let name: String = t.chars().take_while(|c| !c.is_whitespace()).collect();
        let rest = t[name.len()..].trim_start();
        let Some(q) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            continue;
        };
        let Some(end) = rest[1..].find(q) else { break };
        ents.push((format!("&{name};"), rest[1..1 + end].to_string()));
    }
    let mut body = src[e..].to_string();
    for (k, v) in &ents {
        body = body.replace(k, v);
    }
    format!("{}{body}", &src[..e])
}

fn check_file(r: &mut Report, f: &Path) {
    r.files += 1;
    let Ok(src) = std::fs::read_to_string(f) else {
        return;
    };
    let src_ents = expand_entities(&src);
    if src_ents != src {
        r.intentional.push((
            "DTD internal entities",
            f.display().to_string(),
            "expanded".into(),
        ));
    }
    match (legacy::parse_xml(&src_ents), super::read_xml(&src)) {
        (Some(o), Some(n)) => cmp_tree(r, f, &o, &n),
        (None, None) => {}
        (o, n) => r.diff(
            "xml accepted",
            f,
            format!("old {} new {}", o.is_some(), n.is_some()),
        ),
    }
    let p = f.to_str().unwrap();
    let qt_src = qt_keywords(&qt_rgb_percent(&src_ents));
    let tmp = std::env::temp_dir().join(format!("qsg-equiv-{}.svg", std::process::id()));
    let old_p = if qt_src != src {
        std::fs::write(&tmp, &qt_src).unwrap();
        tmp.to_str().unwrap()
    } else {
        p
    };
    let styled = src.contains("current-color-scheme");
    for scheme in [None, Some(SCHEME)].into_iter().take(1 + styled as usize) {
        let (o, n) = (
            legacy::Doc::load_styled(old_p, scheme),
            super::Doc::load_styled(p, scheme),
        );
        let (o, n) = match (o, n) {
            (Ok(o), Ok(n)) => (o, n),
            (Err(_), Err(_)) => continue,
            (o, n) => {
                r.diff(
                    "load",
                    f,
                    format!("old ok {} new ok {}", o.is_ok(), n.is_ok()),
                );
                continue;
            }
        };
        if o.default_size() != n.default_size() {
            r.diff(
                "size",
                f,
                format!("{:?} vs {:?}", o.default_size(), n.default_size()),
            );
        }
    }
    if old_p != p {
        let _ = std::fs::remove_file(&tmp);
    }
}

fn run(files: &[PathBuf]) -> Report {
    let mut r = Report::default();
    for f in files {
        check_file(&mut r, f);
    }
    r
}

#[test]
fn ri_assets_identical() {
    let files = ri_assets();
    assert!(
        files.len() > 200,
        "qml assets missing ({} svgs)",
        files.len()
    );
    let r = run(&files);
    elog::eprintln!("QML assets: {}", r.summary());
    assert!(r.diffs.is_empty(), "{}", r.summary());
}

#[test]
fn system_icon_sample_identical() {
    let files = system_icons(400);
    let r = run(&files);
    elog::eprintln!("system icon sample: {}", r.summary());
    assert!(r.diffs.is_empty(), "{}", r.summary());
}

#[test]
#[cfg_attr(
    not(feature = "live"),
    ignore = "live: --features live, docs/benchmarks.md"
)]
fn all_system_icons_identical() {
    let files = all_system_icons();
    let r = run(&files);
    elog::eprintln!("all system icons: {}", r.summary());
    assert!(r.diffs.is_empty(), "{}", r.summary());
}
