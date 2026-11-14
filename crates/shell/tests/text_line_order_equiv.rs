//! The lines of a Text that has right-to-left characters come out as Qt sets them. QQuickText
//! lays all the lines of a plain text out as one paragraph, a `\n` a line separator in it
//! (QQuickTextPrivate::setupTextLayout): the paragraph's direction is the one of the first
//! character of the whole text that has a direction, and every line is the paragraph's items
//! on it, reordered (QTextEngine::itemize, QTextLine::draw). Ours shaped every line as a
//! paragraph of its own: the line "abc שלום" of a text that starts with a Hebrew word came out
//! left to right, the Latin word on the left.
//! Compared line by line (advance, then family, glyph index and position of every glyph) with
//! Qt itself: a QTextLayout under design metrics, as Qt Quick's Text lays out, through the
//! PySide6 of ri-testenv.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

/// The Python with PySide6 in ri-testenv (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";

/// For every case of the JSON file in argv[1] (`[family, text, width]`, width 0: not wrapped):
/// the number of lines, then one line per line: the advance of its text, then every glyph as
/// `family|glyph|x|y` in 1/64 px from the line's left end and baseline.
const QT_LINES: &str = r#"
import json, sys
from PySide6.QtGui import QGuiApplication, QFont, QTextLayout, QTextOption
app = QGuiApplication(sys.argv[:1])
for family, text, width in json.load(open(sys.argv[1], encoding="utf-8")):
    font = QFont(family)
    font.setPixelSize(16)
    layout = QTextLayout(text.replace("\n", "\u2028"), font)
    option = QTextOption()
    option.setUseDesignMetrics(True)
    option.setWrapMode(QTextOption.WrapMode.WordWrap if width else QTextOption.WrapMode.NoWrap)
    layout.setTextOption(option)
    layout.beginLayout()
    lines = []
    while True:
        line = layout.createLine()
        if not line.isValid():
            break
        line.setLineWidth(width if width else 1e9)
        lines.append(line)
    layout.endLayout()
    print(len(lines))
    for line in lines:
        glyphs = []
        for run in line.glyphRuns():
            name = run.rawFont().familyName()
            for glyph, pos in zip(run.glyphIndexes(), run.positions()):
                x, y = round(pos.x() * 64), round((pos.y() - line.y() - line.ascent()) * 64)
                glyphs.append(f"{name}|{glyph}|{x}|{y}")
        print(round(line.horizontalAdvance() * 64), *glyphs, sep="\t")
"#;

const FAMILIES: [&str; 3] = ["Noto Sans", "DejaVu Sans", "Noto Naskh Arabic"];

/// 0: the lines are only those the text's `\n` make.
const WIDTHS: [u32; 4] = [0, 60, 100, 160];

/// Texts of both directions that take several lines: a line of a right-to-left text that
/// starts with a Latin word and one of a left-to-right text that starts with a Hebrew one,
/// numbers, brackets and punctuation at the ends of lines, lines a `\n` ends.
const TEXTS: [&str; 20] = [
    "שלום abc def עולם",
    "abc שלום עולם def ghi",
    "שלום עולם abc def ghi jkl טוב מאוד",
    "the quick brown fox jumps over the lazy dog",
    "זהו טקסט ארוך בעברית שנשבר לכמה שורות",
    "هذا نص عربي طويل ينكسر إلى عدة أسطر",
    "هذا نص with English words في الوسط and more",
    "price 100 שקלים or 200 דולר (maybe) more",
    "מחיר 100 shekels או 200 dollars (אולי) עוד",
    "abc (שלום עולם) def [טוב מאוד] ghi",
    "שלום (abc def) עולם [ghi jkl] טוב",
    "one שתיים three ארבע five שש seven שמונה",
    "رقم 123 456 و 789 ثم abc 12 34",
    "שלום, world! מה? fine. טוב: yes; לא",
    "שלום\nabc def\nabc עולם",
    "abc\nשלום עולם\nשלום def",
    "שלום עולם\nabc שלום, def!\n123 abc",
    "عربي\n123 abc\nabc 123",
    "first line\nשורה שנייה ארוכה with words\nthird",
    "שורה ראשונה\nsecond long line עם מילים\nשלישית",
];

/// (advance in 1/64 px, glyphs as (family, glyph, x, y) in 1/64 px, sorted)
type Line = (i64, Vec<(String, u32, i64, i64)>);

fn cases() -> Vec<(&'static str, &'static str, u32)> {
    let mut cases = Vec::new();
    for family in FAMILIES {
        for text in TEXTS {
            cases.extend(WIDTHS.map(|width| (family, text, width)));
        }
    }
    cases
}

fn qt_lines(dir: &std::path::Path) -> Vec<Vec<Line>> {
    let (script, file) = (dir.join("lines.py"), dir.join("cases.json"));
    std::fs::write(&script, QT_LINES).unwrap();
    std::fs::write(&file, serde_json::to_string(&cases()).unwrap()).unwrap();
    let out = std::process::Command::new(PYTHON)
        .arg(&script)
        .arg(&file)
        .env("QT_QPA_PLATFORM", "offscreen")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{PYTHON} failed:\n{log}");
    let out = String::from_utf8(out.stdout).unwrap();
    let mut rows = out.lines();
    let mut laid = Vec::new();
    while let Some(count) = rows.next() {
        let lines = (0..count.parse().unwrap())
            .map(|_| {
                let mut fields = rows.next().unwrap().split('\t');
                let advance = fields.next().unwrap().parse().unwrap();
                let mut glyphs: Vec<_> = fields
                    .map(|g| {
                        let f: Vec<&str> = g.split('|').collect();
                        let n = |i: usize| f[i].parse::<i64>().unwrap();
                        (f[0].to_string(), n(1) as u32, n(2), n(3))
                    })
                    .collect();
                glyphs.sort();
                (advance, glyphs)
            })
            .collect();
        laid.push(lines);
    }
    assert_eq!(laid.len(), cases().len(), "Qt printed:\n{log}");
    laid
}

fn our_lines() -> Vec<Vec<Line>> {
    let mut source = String::from("import QtQuick\nItem {\n");
    for (i, (family, text, width)) in cases().iter().enumerate() {
        let wrap = match width {
            0 => String::new(),
            _ => format!("; wrapMode: Text.WordWrap; width: {width}"),
        };
        source += &format!(
            "    property Item item{i}: Text {{ font.family: {family:?}; font.pixelSize: 16; \
             text: {}{wrap} }}\n",
            serde_json::to_string(text).unwrap()
        );
    }
    source += "}\n";
    let mut rt = Runtime::new(None);
    let fonts = std::rc::Rc::new(std::cell::RefCell::new(scene::Fonts::default()));
    {
        let fonts = fonts.clone();
        rt.world.borrow_mut().text_measure = Some(Box::new(move |w, i| {
            if let Ok(mut f) = fonts.try_borrow_mut() {
                scene::measure_text(w, &mut f, i);
            }
        }));
    }
    let root = rt.load_str(&source).unwrap();
    rt.settle();
    scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, [400.0, 300.0]);
    rt.settle();
    let mut w = rt.world.borrow_mut();
    (0..cases().len())
        .map(|i| {
            let Some(Value::Item(t)) = w.read(root, &format!("item{i}")) else {
                panic!("no item{i}");
            };
            scene::text_line_glyphs(&mut w, &mut fonts.borrow_mut(), t)
                .into_iter()
                .map(|(advance, mut glyphs)| {
                    glyphs.sort();
                    (advance, glyphs)
                })
                .collect()
        })
        .collect()
}

#[test]
fn lines_of_text_with_both_directions_match_qt() {
    if !std::path::Path::new(PYTHON).exists() {
        testenv::unavailable(format_args!("`{PYTHON}` (PySide6) is not installed"));
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (qt, ours) = (qt_lines(dir.path()), our_lines());
    let cases = cases();
    let mut wrong = Vec::new();
    for ((family, text, width), (qt, ours)) in cases.iter().zip(qt.iter().zip(&ours)) {
        if qt != ours {
            let line = (0..qt.len().max(ours.len()))
                .find(|&k| qt.get(k) != ours.get(k))
                .unwrap();
            wrong.push(format!(
                "{family} width {width} {text:?}: {} lines, Qt {}; line {line}:\n  ours {:?}\n  \
                 Qt   {:?}",
                ours.len(),
                qt.len(),
                ours.get(line),
                qt.get(line)
            ));
        }
    }
    elog::eprintln!(
        "{} texts compared with Qt, {} differ",
        cases.len(),
        wrong.len()
    );
    assert!(
        wrong.is_empty(),
        "{} of {} texts differ from Qt:\n{}",
        wrong.len(),
        cases.len(),
        wrong.join("\n")
    );
}
