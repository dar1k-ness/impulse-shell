//! Characters HarfBuzz composes, decomposes or mirrors while shaping come out as Qt shapes
//! them. HarfBuzz asks the Unicode functions of its buffer for that, and QTextEngine sets Qt's
//! own on every buffer (hb_qt_get_unicode_funcs, qharfbuzzng.cpp): the data of Qt's Unicode
//! version, and a compose function that takes the NFC of the pair, so "ᾀ" + U+0300 is the one
//! glyph of "ᾂ" in a family that has it and lacks the iota subscript U+0345 it would take to
//! put the pair in canonical order first. Ours shaped with HarfBuzz's tables, of the Unicode
//! version HarfBuzz was built with, and drew two glyphs.
//! Compared glyph by glyph (family, glyph index, position) and by width with Qt itself, in
//! every family fontconfig lists: each pair Qt's function composes, each character it
//! decomposes (alone and under a mark, which is when HarfBuzz decomposes a character the font
//! has) and each character with a mirrored one in a right-to-left run, where the family has
//! the characters of the text. The texts are of `qt_hb/qt.txt`, Qt's answers.

use qsg::text::{FontStack, fc_match_file};
use std::collections::BTreeMap;

/// The Python with PySide6 in ri-testenv (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";

const QT: &str = include_str!("qt_hb/qt.txt");

/// Marks put after a character the function decomposes: two of any script, a Hebrew point
/// and a musical one.
const MARKS: [char; 4] = ['\u{301}', '\u{323}', '\u{5B8}', '\u{1D17B}'];

/// For each family of the file in argv[2] at argv[1] pixels, a line `family NAME` and then,
/// for each text of the file in argv[3] whose characters the family's font has (U+202E aside),
/// the text's number, the line's width and every glyph as `family|glyph|x|y` in 1/64 px from
/// the line's left end and baseline.
const QT_GLYPHS: &str = r#"
import sys
from PySide6.QtGui import QGuiApplication, QFont, QRawFont, QTextLayout, QTextOption
app = QGuiApplication(sys.argv[:1])
texts = open(sys.argv[3], encoding="utf-8").read().split("\n")
chars = sorted({ord(c) for text in texts for c in text} - {0x202E})
option = QTextOption()
option.setUseDesignMetrics(True)
for name in open(sys.argv[2], encoding="utf-8").read().split("\n"):
    font = QFont(name)
    font.setPixelSize(int(sys.argv[1]))
    raw = QRawFont.fromFont(font)
    has = {c for c in chars if raw.supportsCharacter(c)}
    has.add(0x202E)
    print("family", name)
    for i, text in enumerate(texts):
        if not all(ord(c) in has for c in text):
            continue
        layout = QTextLayout(text, font)
        layout.setTextOption(option)
        layout.beginLayout()
        line = layout.createLine()
        line.setLineWidth(1e9)
        layout.endLayout()
        glyphs = []
        for run in layout.glyphRuns():
            family = run.rawFont().familyName()
            for glyph, pos in zip(run.glyphIndexes(), run.positions()):
                x, y = round(pos.x() * 64), round((pos.y() - line.ascent()) * 64)
                glyphs.append(f"{family}|{glyph}|{x}|{y}")
        print(i, line.naturalTextWidth(), *glyphs, sep="\t")
"#;

/// (what the text checks, the text)
fn texts() -> Vec<(&'static str, String)> {
    let c = |hex: &str| char::from_u32(u32::from_str_radix(hex, 16).unwrap()).unwrap();
    let mut texts = vec![];
    for line in QT.lines() {
        let w: Vec<&str> = line.split(' ').collect();
        match w[0] {
            "compose" => texts.push(("compose", format!("{}{}", c(w[1]), c(w[2])))),
            "decompose" => {
                texts.push(("decompose", c(w[1]).to_string()));
                for mark in MARKS {
                    texts.push(("decompose", format!("{}{mark}", c(w[1]))));
                }
            }
            "mirror" => texts.push(("mirror", format!("\u{202E}{}", c(w[1])))),
            _ => {}
        }
    }
    texts
}

/// The first name of every font fontconfig has.
fn families() -> Vec<String> {
    let out = std::process::Command::new("fc-list")
        .args([":", "family"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let mut names: Vec<String> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| l.split(',').next().unwrap().replace("\\-", "-"))
        .filter(|l| !l.is_empty())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// (line width in px, glyphs as (family, glyph, x, y) in 1/64 px, sorted)
type Line = (f64, Vec<(String, u32, i64, i64)>);

/// For each family, the lines of the texts its font has, by the text's number.
fn qt_lines(
    dir: &std::path::Path,
    families: &[String],
    texts: &[(&str, String)],
    px: u32,
) -> Vec<BTreeMap<usize, Line>> {
    let (script, names, file) = (
        dir.join("glyphs.py"),
        dir.join("families"),
        dir.join("texts"),
    );
    std::fs::write(&script, QT_GLYPHS).unwrap();
    std::fs::write(&names, families.join("\n")).unwrap();
    let all: Vec<&str> = texts.iter().map(|t| t.1.as_str()).collect();
    std::fs::write(&file, all.join("\n")).unwrap();
    let out = std::process::Command::new(PYTHON)
        .arg(&script)
        .arg(px.to_string())
        .args([&names, &file])
        .env("QT_QPA_PLATFORM", "offscreen")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{PYTHON} failed:\n{log}");
    let mut lines: Vec<BTreeMap<usize, Line>> = vec![];
    for l in String::from_utf8(out.stdout).unwrap().lines() {
        if l.starts_with("family ") {
            lines.push(BTreeMap::new());
            continue;
        }
        let mut fields = l.split('\t');
        let text = fields.next().unwrap().parse().unwrap();
        let width = fields.next().unwrap().parse().unwrap();
        let mut glyphs: Vec<_> = fields
            .map(|g| {
                let f: Vec<&str> = g.split('|').collect();
                let n = |i: usize| f[i].parse::<i64>().unwrap();
                (f[0].to_string(), n(1) as u32, n(2), n(3))
            })
            .collect();
        glyphs.sort();
        lines.last_mut().unwrap().insert(text, (width, glyphs));
    }
    assert_eq!(lines.len(), families.len(), "Qt printed:\n{log}");
    lines
}

fn our_line(stack: &FontStack, text: &str) -> Line {
    let line = stack.shape(text);
    let mut glyphs: Vec<_> = line
        .glyphs
        .iter()
        .map(|&(f, g, x, y)| (stack.font(f).unwrap().family(), g, x, y))
        .collect();
    glyphs.sort();
    (stack.natural_width(&line) as f64 / 64.0, glyphs)
}

#[test]
fn composed_decomposed_and_mirrored_characters_match_qt() {
    if !std::path::Path::new(PYTHON).exists() {
        testenv::unavailable(format_args!("`{PYTHON}` (PySide6) is not installed"));
        return;
    }
    const PX: u32 = 16;
    let dir = tempfile::tempdir().unwrap();
    let (families, texts) = (families(), texts());
    let qt = qt_lines(dir.path(), &families, &texts, PX);
    let (mut cases, mut wrong, mut table) = (0, vec![], String::new());
    for (family, qt) in families.iter().zip(&qt) {
        if qt.is_empty() {
            continue;
        }
        let path = fc_match_file(family).unwrap();
        let stack = FontStack::from_path(&path, PX, 400.0, &[]).unwrap();
        // what the texts check -> (texts, texts that differ)
        let mut counts: BTreeMap<&str, (u32, u32)> = BTreeMap::new();
        for (&i, qt) in qt {
            let (kind, text) = &texts[i];
            let ours = our_line(&stack, text);
            let count = counts.entry(kind).or_default();
            cases += 1;
            count.0 += 1;
            if *qt != ours {
                count.1 += 1;
                wrong.push(format!(
                    "{family} {kind} {:?}:\n  ours {ours:?}\n  Qt   {qt:?}",
                    text.escape_unicode().to_string()
                ));
            }
        }
        let row: Vec<String> = counts
            .iter()
            .map(|(kind, (texts, differ))| format!("{kind} {texts} differ {differ}"))
            .collect();
        table += &format!("{family}: {}\n", row.join(", "));
    }
    elog::eprintln!(
        "{table}{cases} lines of {} families compared with Qt, {} differ",
        families.len(),
        wrong.len()
    );
    assert!(
        wrong.is_empty(),
        "{} of {cases} lines differ from Qt:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
