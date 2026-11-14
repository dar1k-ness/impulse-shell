//! A line mixing right-to-left and left-to-right text comes out as Qt lays it out. Qt resolves
//! the Unicode Bidirectional Algorithm over the line, splits it where the level, the script or
//! the font changes, shapes each piece in its own direction and sets the pieces in visual order
//! (QTextEngine::itemize, shapeText, bidiReorder). Ours shaped each font's stretch in the
//! direction HarfBuzz guessed from its first letter and set the stretches left to right in the
//! order of the text: "שלום abc" drew as "cba םולש".
//! Compared glyph by glyph (family, glyph index, position) and by width with Qt itself: a
//! QTextLayout under design metrics, as Qt Quick's Text lays out, through the PySide6 of
//! ri-testenv.

use qsg::text::{FontStack, fc_match_file};

/// The Python with PySide6 in ri-testenv (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";

/// One line per text of the file in argv[3], for the family argv[1] at argv[2] pixels: the
/// line's width, then every glyph as `family|glyph|x|y` in 1/64 px from the line's left end and
/// baseline.
const QT_GLYPHS: &str = r#"
import sys
from PySide6.QtGui import QGuiApplication, QFont, QTextLayout, QTextOption
app = QGuiApplication(sys.argv[:1])
font = QFont(sys.argv[1])
font.setPixelSize(int(sys.argv[2]))
for text in open(sys.argv[3], encoding="utf-8").read().split("\n"):
    layout = QTextLayout(text, font)
    option = QTextOption()
    option.setUseDesignMetrics(True)
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
    print(line.naturalTextWidth(), *glyphs, sep="\t")
"#;

/// A family with Hebrew and Arabic letters of its own; two without, one of them fixed-pitch
/// (every right-to-left word goes to a fallback family); and three Arabic ones, where Latin and
/// Hebrew are the fallback and so is ASCII punctuation. Qt takes a character the family lacks
/// from a fallback list sorted for the language of the script item the character is in
/// (QFontconfigDatabase::fallbacksForFamily, specialLanguages: "he", "ar"): for the Arabic
/// families fontconfig puts DejaVu Sans before Noto Sans under those languages and after it
/// otherwise, so a bracket or a comma inside Hebrew or Arabic text is DejaVu Sans's and one
/// inside Latin text is Noto Sans's.
const FAMILIES: [&str; 6] = [
    "DejaVu Sans",
    "Noto Sans",
    "Noto Sans Mono",
    "Noto Sans Arabic",
    "Noto Naskh Arabic",
    "Noto Kufi Arabic",
];

/// Lines of one direction, as before the change, and lines mixing the two: Hebrew or Arabic
/// with Latin words, with digits, with brackets and punctuation, with both scripts, with
/// combining marks, with explicit direction marks, and Latin lines holding a right-to-left
/// word. Then lines mixing scripts of one direction in one font, and lines with combining
/// marks stacked on a letter and ending in one: the width of a line counts the right bearing
/// of its last letter, not of the mark on it (QTextLine::naturalTextWidth), and DejaVu Sans
/// attaches its marks at points of the glyph outlines, which Qt reads from FreeType with y
/// pointing up (`x̂́` has both marks at the same height, far above the letter). The last lines
/// hold code points whose class in the algorithm is another in Unicode 17, which Qt 6.12
/// reads, than in Unicode 16 (a new Arabic letter, new marks, new symbols, the code points
/// with no character behind them), and code points with no character where Qt's class is not
/// the Unicode Character Database's (R, not AL: digits after one stay European, `1-2` is not
/// turned around); each line is laid out differently under the other class.
const TEXTS: [&str; 64] = [
    "hello world",
    "12:34",
    "Привет, мир!",
    "Battery 87%",
    "AV. To (fi) -> 1/2",
    "שלום עולם",
    "مرحبا بالعالم",
    "שלום abc",
    "abc שלום",
    "שלום 123",
    "123 שלום",
    "abc שלום def",
    "שלום abc עולם",
    "שלום (abc) עולם",
    "abc (שלום) def",
    "שלום, עולם!",
    "שלום - abc: 12.5%",
    "مرحبا abc",
    "abc مرحبا",
    "abc مرحبا 123",
    "مرحبا 123 abc",
    "السعر 100 USD",
    "السعر 12,5 - 3/4",
    "١٢٣ مرحبا 456",
    "ab שלום مرحبا cd",
    "مرحبا שלום",
    "Wi-Fi: الشبكة (5 GHz)",
    "فيروز - نسم علينا الهوى.mp3",
    "עומר אדם - תל אביב (feat. Arisa) [2013]",
    "Firefox — ויקיפדיה",
    "שָׁלוֹם abc",
    "مَرْحَبًا abc",
    "a\u{200F}! b",
    "אב\u{200E} cd",
    "\u{202E}abc\u{202C} def",
    "abc \u{2067}שלום 12\u{2069} def",
    "שלום 😀 abc",
    "abc 😀 שלום",
    "Привет world",
    "Ελληνικά and Кириллица, 日本語 too",
    "Zu\u{308}\u{304} q\u{323} x",
    "x\u{302}\u{301}",
    "ab x\u{302}\u{301} cd",
    "Zu\u{308}\u{304} q\u{323}x\u{302}\u{301}",
    "abc q\u{323}",
    "g\u{303}\u{323}\u{301} k\u{302}",
    "abc שָׁ",
    "مرحبً",
    "\u{88F} 1-2",
    "\u{10EC5} 1-2",
    "\u{10ED0} abc",
    "\u{FBC3} abc",
    "\u{FD90} abc 12",
    "\u{1ACF}\u{5D0}\u{5D1} abc",
    "\u{11B60}\u{5D0}\u{5D1} abc",
    "\u{1E6E3}\u{5D0}\u{5D1} abc",
    "\u{5D0} \u{2B96}1 \u{5D1}",
    "\u{5D0} \u{1F6D8}1 \u{5D1}",
    "\u{5D0} \u{1CCFA}1 \u{5D1}",
    "\u{5D0} \u{E0080}1 \u{5D1}",
    "\u{10EC9} 1-2",
    "\u{86B} 1-2",
    "\u{10D28} 1-2",
    "\u{1ECB5} 1-2",
];

/// (line width in px, glyphs as (family, glyph, x, y) in 1/64 px, sorted)
type Line = (f64, Vec<(String, u32, i64, i64)>);

fn qt_lines(dir: &std::path::Path, family: &str, px: u32) -> Vec<Line> {
    let (script, texts) = (dir.join("glyphs.py"), dir.join("texts"));
    std::fs::write(&script, QT_GLYPHS).unwrap();
    std::fs::write(&texts, TEXTS.join("\n")).unwrap();
    let out = std::process::Command::new(PYTHON)
        .arg(&script)
        .args([family, &px.to_string()])
        .arg(&texts)
        .env("QT_QPA_PLATFORM", "offscreen")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{PYTHON} failed:\n{log}");
    let lines: Vec<Line> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| {
            let mut fields = l.split('\t');
            let width = fields.next().unwrap().parse().unwrap();
            let mut glyphs: Vec<_> = fields
                .map(|g| {
                    let f: Vec<&str> = g.split('|').collect();
                    let n = |i: usize| f[i].parse::<i64>().unwrap();
                    (f[0].to_string(), n(1) as u32, n(2), n(3))
                })
                .collect();
            glyphs.sort();
            (width, glyphs)
        })
        .collect();
    assert_eq!(lines.len(), TEXTS.len(), "Qt printed:\n{log}");
    lines
}

fn our_lines(family: &str, px: u32) -> Vec<Line> {
    let path = fc_match_file(family).unwrap();
    let stack = FontStack::from_path(&path, px, 400.0, &[]).unwrap();
    TEXTS
        .iter()
        .map(|text| {
            let line = stack.shape(text);
            let mut glyphs: Vec<_> = line
                .glyphs
                .iter()
                .map(|&(f, g, x, y)| (stack.font(f).unwrap().family(), g, x, y))
                .collect();
            glyphs.sort();
            (stack.natural_width(&line) as f64 / 64.0, glyphs)
        })
        .collect()
}

#[test]
fn mixed_direction_lines_match_qt() {
    if !std::path::Path::new(PYTHON).exists() {
        testenv::unavailable(format_args!("`{PYTHON}` (PySide6) is not installed"));
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut cases, mut wrong) = (0, vec![]);
    for family in FAMILIES {
        for px in [13, 16] {
            let (qt, ours) = (qt_lines(dir.path(), family, px), our_lines(family, px));
            for (text, (qt, ours)) in TEXTS.iter().zip(qt.iter().zip(&ours)) {
                cases += 1;
                if qt != ours {
                    wrong.push(format!(
                        "{family} {px}px {text:?}:\n  ours {ours:?}\n  Qt   {qt:?}"
                    ));
                }
            }
        }
    }
    elog::eprintln!("{cases} lines compared with Qt, {} differ", wrong.len());
    assert!(
        wrong.is_empty(),
        "{} of {cases} lines differ from Qt:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

/// For the family argv[1] at argv[2] pixels, every text of the file in argv[3] wrapped at words
/// to argv[4] pixels: the number of lines, then one line per line: where it starts in the text
/// and how long it is in UTF-16 units, the advance of its text, then its glyphs as in QT_GLYPHS.
const QT_WRAPPED: &str = r#"
import sys
from PySide6.QtGui import QGuiApplication, QFont, QTextLayout, QTextOption
app = QGuiApplication(sys.argv[:1])
font = QFont(sys.argv[1])
font.setPixelSize(int(sys.argv[2]))
for text in open(sys.argv[3], encoding="utf-8").read().split("\n"):
    layout = QTextLayout(text, font)
    option = QTextOption()
    option.setUseDesignMetrics(True)
    option.setWrapMode(QTextOption.WrapMode.WordWrap)
    layout.setTextOption(option)
    layout.beginLayout()
    lines = []
    while True:
        line = layout.createLine()
        if not line.isValid():
            break
        line.setLineWidth(float(sys.argv[4]))
        lines.append(line)
    layout.endLayout()
    print(len(lines))
    for line in lines:
        glyphs = []
        for run in line.glyphRuns():
            family = run.rawFont().familyName()
            for glyph, pos in zip(run.glyphIndexes(), run.positions()):
                x, y = round(pos.x() * 64), round((pos.y() - line.y() - line.ascent()) * 64)
                glyphs.append(f"{family}|{glyph}|{x}|{y}")
        print(line.textStart(), line.textLength(), line.horizontalAdvance(), *glyphs, sep="\t")
"#;

/// Paragraphs of both directions that take several lines: a line of a right-to-left paragraph
/// that starts with a Latin word, one of a left-to-right paragraph that starts with a Hebrew
/// one, numbers, brackets and marks that stand at the ends of lines.
const PARAGRAPHS: [&str; 16] = [
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
    "אחת two שלוש four חמש six שבע eight",
    "رقم 123 456 و 789 ثم abc 12 34",
    "abc 12 34 שלום 56 78 def 90",
    "שלום, world! מה? fine. טוב: yes; לא",
];

/// (start and length in UTF-16 units, the line)
type Wrapped = ((usize, usize), Line);

fn qt_wrapped(dir: &std::path::Path, family: &str, px: u32, width: u32) -> Vec<Vec<Wrapped>> {
    let (script, texts) = (dir.join("wrapped.py"), dir.join("paragraphs"));
    std::fs::write(&script, QT_WRAPPED).unwrap();
    std::fs::write(&texts, PARAGRAPHS.join("\n")).unwrap();
    let out = std::process::Command::new(PYTHON)
        .arg(&script)
        .args([family, &px.to_string()])
        .arg(&texts)
        .arg(width.to_string())
        .env("QT_QPA_PLATFORM", "offscreen")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{PYTHON} failed:\n{log}");
    let out = String::from_utf8(out.stdout).unwrap();
    let mut rows = out.lines();
    let mut paragraphs = Vec::new();
    while let Some(count) = rows.next() {
        let lines = (0..count.parse().unwrap())
            .map(|_| {
                let mut fields = rows.next().unwrap().split('\t');
                let mut number = || fields.next().unwrap();
                let at = (number().parse().unwrap(), number().parse().unwrap());
                let width = number().parse().unwrap();
                let mut glyphs: Vec<_> = fields
                    .map(|g| {
                        let f: Vec<&str> = g.split('|').collect();
                        let n = |i: usize| f[i].parse::<i64>().unwrap();
                        (f[0].to_string(), n(1) as u32, n(2), n(3))
                    })
                    .collect();
                glyphs.sort();
                (at, (width, glyphs))
            })
            .collect();
        paragraphs.push(lines);
    }
    assert_eq!(paragraphs.len(), PARAGRAPHS.len(), "Qt printed:\n{log}");
    paragraphs
}

/// A line of a paragraph that takes several is set in the paragraph's direction, with the
/// levels its characters have in the paragraph (QTextEngine::itemize runs once over the
/// paragraph; QTextLine::draw reorders the items of the line). Shaped on its own, the line
/// "abc def" of a Hebrew paragraph is fine, but "abc שלום" of it came out as a left-to-right
/// line, the Latin word on the wrong side.
#[test]
fn lines_of_a_wrapped_paragraph_match_qt() {
    if !std::path::Path::new(PYTHON).exists() {
        testenv::unavailable(format_args!("`{PYTHON}` (PySide6) is not installed"));
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut cases, mut wrong, mut alone) = (0, vec![], 0);
    for family in FAMILIES {
        let path = fc_match_file(family).unwrap();
        let stack = FontStack::from_path(&path, 16, 400.0, &[]).unwrap();
        let ours = |line: qsg::text::ShapedLine| {
            let mut glyphs: Vec<_> = line
                .glyphs
                .iter()
                .map(|&(f, g, x, y)| (stack.font(f).unwrap().family(), g, x, y))
                .collect();
            glyphs.sort();
            (line.advance as f64 / 64.0, glyphs)
        };
        for width in [60, 100, 160] {
            let qt = qt_wrapped(dir.path(), family, 16, width);
            for (text, lines) in PARAGRAPHS.iter().zip(&qt) {
                // Byte of every UTF-16 unit.
                let mut bytes: Vec<usize> = text
                    .char_indices()
                    .flat_map(|(b, c)| std::iter::repeat_n(b, c.len_utf16()))
                    .collect();
                bytes.push(text.len());
                for ((start, len), qt) in lines {
                    let (a, b) = (bytes[*start], bytes[start + len]);
                    let b = a + text[a..b].trim_end_matches(' ').len();
                    cases += 1;
                    let line = ours(stack.shape_part(text, a..b));
                    if &line != qt {
                        wrong.push(format!(
                            "{family} width {width} {text:?} line {:?}:\n  ours {line:?}\n  Qt   {qt:?}",
                            &text[a..b]
                        ));
                    }
                    alone += (&ours(stack.shape(&text[a..b])) != qt) as usize;
                }
            }
        }
    }
    elog::eprintln!(
        "{cases} lines of wrapped paragraphs compared with Qt, {} differ ({alone} shaped on \
         their own)",
        wrong.len()
    );
    assert!(
        wrong.is_empty(),
        "{} of {cases} lines differ from Qt:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
