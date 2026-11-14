//! The caret of a line stands where Qt puts it, and a point of the line picks the caret
//! position Qt picks: QTextLine::cursorToX and QTextLine::xToCursor, for lines of one
//! direction, lines mixing right-to-left and left-to-right text, ligatures and combining marks.
//! Ours measured the text before the caret on its own, which is the wrong place in any line
//! that is not set left to right in the order of its text: in "שלום abc" the caret after "ש"
//! stood at the left end of the line, Qt has it one letter from the right end.
//! The same of the lines of a paragraph broken at a width, which Qt sets in the paragraph's
//! direction whatever the line starts with, and whose white space at the break hangs out of
//! the line's text.
//! Compared with Qt itself: a QTextLayout under design metrics, as Qt Quick's TextInput lays
//! out, through the PySide6 of ri-testenv.

use qsg::text::{FontStack, fc_match_file};

/// The Python with PySide6 in ri-testenv (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";

/// Three lines per text of the file in argv[3], for the family argv[1] at argv[2] pixels: the
/// UTF-16 units a grapheme starts at; cursorToX of every unit and the end, in 1/64 px; and
/// xToCursor of every half pixel from 3 px left of the line to 3 px right of it.
const QT_CARETS: &str = r#"
import sys
from PySide6.QtCore import QTextBoundaryFinder
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
    units = len(text.encode("utf-16-le")) // 2
    finder = QTextBoundaryFinder(QTextBoundaryFinder.BoundaryType.Grapheme, text)
    bounds = [0]
    while finder.toNextBoundary() >= 0:
        bounds.append(finder.position())
    print(*bounds)
    xs = []
    for i in range(units + 1):
        x = line.cursorToX(i)
        xs.append(round((x[0] if isinstance(x, tuple) else x) * 64))
    print(*xs)
    print(*[line.xToCursor(h / 2) for h in range(-6, int(line.naturalTextWidth() * 2) + 8)])
"#;

const FAMILIES: [&str; 3] = ["DejaVu Sans", "Noto Sans", "Noto Naskh Arabic"];

/// Lines of one direction; lines whose words change direction, with digits, brackets, both
/// right-to-left scripts, direction marks and emoji; ligatures of Latin ("fi", "ffl") and of
/// Arabic (lam-alef), which the caret steps into in Latin and over in Arabic; combining marks,
/// inside which no caret stands; tabs, each as wide as the way to the next tab stop of 80 px.
const TEXTS: [&str; 52] = [
    "hello world",
    "AV. To (fi) -> 1/2",
    "office waffle fi",
    "Привет, мир!",
    "שלום עולם",
    "مرحبا بالعالم",
    "السلام عليكم",
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
    "עומר אדם - תל אביב (feat. Arisa) [2013]",
    "Firefox — ויקיפדיה",
    "שָׁלוֹם abc",
    "مَرْحَبًا abc",
    "a\u{200F}! b",
    "אב\u{200E} cd",
    "\u{202E}abc fi\u{202C} def",
    "abc \u{2067}שלום 12\u{2069} def",
    "שלום 😀 abc",
    "abc 😀 שלום",
    "Ελληνικά and Кириллица, 日本語 too",
    "Zu\u{308}\u{304} q\u{323} x",
    "ab x\u{302}\u{301} cd",
    "abc שָׁ",
    "مرحبً",
    " שלום ",
    "a\tb",
    "\tfn main() {",
    "ab\t\tcd",
    "a fairly long line\tb\t",
    "if x:\treturn fi\t# office",
    "abc שלום\tdef",
    "שלום\tabc\tעולם",
    "مرحبا\t123 abc",
    "",
];

/// (grapheme boundaries, cursorToX of every unit, xToCursor of every half pixel), in UTF-16
/// units and 1/64 px.
type Carets = (Vec<usize>, Vec<i64>, Vec<usize>);

fn qt_carets(dir: &std::path::Path, family: &str, px: u32) -> Vec<Carets> {
    let (script, texts) = (dir.join("carets.py"), dir.join("texts"));
    std::fs::write(&script, QT_CARETS).unwrap();
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
    let out = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3 * TEXTS.len(), "Qt printed:\n{out}\n{log}");
    fn numbers<T: std::str::FromStr>(l: &str) -> Vec<T> {
        l.split_whitespace().flat_map(|n| n.parse().ok()).collect()
    }
    lines
        .chunks(3)
        .map(|l| (numbers(l[0]), numbers(l[1]), numbers(l[2])))
        .collect()
}

/// The same of our line, at the `picks` places Qt was asked about; `bounds` are Qt's graphemes.
fn our_carets(
    stack: &FontStack,
    text: &str,
    bounds: &[usize],
    picks: usize,
) -> (Vec<i64>, Vec<usize>) {
    // The UTF-16 unit of every character and the end, and the unit every byte starts at.
    let mut units = vec![];
    let mut unit_at = std::collections::HashMap::new();
    let mut n = 0;
    for (b, c) in text.char_indices() {
        unit_at.insert(b, n);
        units.push(n);
        n += c.len_utf16();
    }
    units.push(n);
    let map = stack.line_map(text, &|b| bounds.contains(&unit_at[&b]));
    // Qt moves a position inside a character to the grapheme after it, as ours does one
    // inside a grapheme.
    let xs = (0..=n)
        .map(|u| map.cursor_to_x(units.partition_point(|&c| c < u)))
        .collect();
    let picks = (-6..picks as i64 - 6)
        .map(|h| units[map.x_to_cursor(h * 32)])
        .collect();
    (xs, picks)
}

#[test]
fn carets_match_qt() {
    if !std::path::Path::new(PYTHON).exists() {
        testenv::unavailable(format_args!("`{PYTHON}` (PySide6) is not installed"));
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut cases, mut wrong) = (0, vec![]);
    for family in FAMILIES {
        let path = fc_match_file(family).unwrap();
        for px in [13, 16] {
            let stack = FontStack::from_path(&path, px, 400.0, &[]).unwrap();
            let qt = qt_carets(dir.path(), family, px);
            for (text, (bounds, qt_xs, qt_picks)) in TEXTS.iter().zip(&qt) {
                cases += 1;
                let (xs, picks) = our_carets(&stack, text, bounds, qt_picks.len());
                if (&xs, &picks) != (qt_xs, qt_picks) {
                    wrong.push(format!(
                        "{family} {px}px {text:?}:\n  ours {xs:?}\n  Qt   {qt_xs:?}\n  ours {picks:?}\n  Qt   {qt_picks:?}"
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

/// For every text of the file in argv[3] laid out in lines of argv[4] pixels, for the family
/// argv[1] at argv[2] pixels, with of argv[5] 1: word wrap, 2:
/// QTextOption::IncludeTrailingSpaces, 4: Qt::AlignRight: the number of its lines, and two
/// lines for each: its first unit, its units, its naturalTextWidth and cursorToX of each of its
/// units and of the end, in 1/64 px; and xToCursor of every half pixel from 3 px left of the
/// line to 3 px right of it.
const QT_LINES: &str = r#"
import sys
from PySide6.QtCore import Qt
from PySide6.QtGui import QGuiApplication, QFont, QTextLayout, QTextOption
app = QGuiApplication(sys.argv[:1])
font = QFont(sys.argv[1])
font.setPixelSize(int(sys.argv[2]))
width = float(sys.argv[4])
flags = int(sys.argv[5])
for text in open(sys.argv[3], encoding="utf-8").read().split("\n"):
    layout = QTextLayout(text, font)
    option = QTextOption()
    option.setUseDesignMetrics(True)
    option.setWrapMode(QTextOption.WrapMode.WordWrap if flags & 1 else QTextOption.WrapMode.NoWrap)
    if flags & 2:
        option.setFlags(QTextOption.Flag.IncludeTrailingSpaces)
    if flags & 4:
        option.setAlignment(Qt.AlignmentFlag.AlignRight)
    layout.setTextOption(option)
    layout.beginLayout()
    lines = []
    while True:
        line = layout.createLine()
        if not line.isValid():
            break
        line.setLineWidth(width)
        lines.append(line)
    layout.endLayout()
    print(len(lines))
    for line in lines:
        a, n = line.textStart(), line.textLength()
        xs = []
        for i in range(a, a + n + 1):
            x = line.cursorToX(i)
            xs.append(round((x[0] if isinstance(x, tuple) else x) * 64))
        print(a, n, round(line.naturalTextWidth() * 64), *xs)
        print(*[line.xToCursor(h / 2) for h in range(-6, int(width * 2) + 8)])
        out = []
        for sa in range(a, a + n):
            for sb in range(sa + 1, a + n + 1):
                rects = selection(line, sa, sb)
                out += [sa, sb, len(rects)]
                for l, r in rects:
                    out += [l, r]
        print(*out)
"#;

/// QQuickTextNodeEngine::addGlyphsInRange and processCurrentLine, for the Python above: the
/// (left, right) in 1/64 px of the rectangles Qt Quick draws behind units `sa..sb` of a line.
/// The glyph runs of the line before, in and after the selection, in the order of their left
/// edges; selected runs next to each other are one rectangle.
const QT_SELECTION: &str = r#"
def selection(line, sa, sb):
    start, length = line.textStart(), line.textLength()
    end, last = start + length, sb - 1
    nodes = []
    def add(a, n, selected):
        for run in line.glyphRuns(a, n):
            rect = run.boundingRect()
            nodes.append((rect.left(), rect.right(), selected))
    if start < sa:
        add(start, min(sa - start, length), False)
    if end > sa:
        a = max(sa, start)
        add(a, min(last - a + 1, end - a), True)
    if start <= last < end:
        add(last + 1, end - last - 1, False)
    nodes.sort(key=lambda node: node[0])
    rects = []
    was = False
    for left, right, selected in nodes:
        if selected and was:
            rects[-1] = (min(rects[-1][0], left), max(rects[-1][1], right))
        elif selected:
            rects.append((left, right))
        was = selected
    return [(round(l * 64), round(r * 64)) for l, r in rects]
"#;

/// Paragraphs of words without a combining mark or a character outside the BMP: a character
/// of them is a UTF-16 unit and a grapheme.
const PARAGRAPHS: [&str; 23] = [
    "hello world  foo bar",
    "office waffle fi fl",
    "שלום עולם שלום עולם",
    "مرحبا بالعالم مرحبا",
    "שלום abc עולם",
    "abc שלום def ghi",
    "abc שלום עולם def",
    "שלום abc def עולם",
    "abc מה שלומך היום def",
    "שלום hello world foo עולם",
    "السعر 100 USD مرحبا",
    "abc مرحبا بالعالم 123 def",
    "Wi-Fi: الشبكة (5 GHz) ok",
    "שלום   abc",
    "abc   שלום",
    "abc שלום ",
    "שלום abc  ",
    " שלום עולם",
    "ab\tcd ef\tgh ij",
    "\tabc def\tx yz",
    "a\t\tb c\t d",
    "שלום\tabc def עולם",
    "abc\tשלום עולם def",
];

/// The rectangles behind units `.0`..`.1` of a line, each (left, right) in 1/64 px.
type Selection = (usize, usize, Vec<(i64, i64)>);

/// (first unit, units, naturalTextWidth, cursorToX of each unit and of the end, xToCursor of
/// every half pixel, the selection of every stretch of the line) of every line of every
/// paragraph.
type Lines = Vec<(usize, usize, i64, Vec<i64>, Vec<usize>, Vec<Selection>)>;

fn qt_lines(dir: &std::path::Path, family: &str, px: u32, width: u32, flags: u32) -> Vec<Lines> {
    let (script, texts) = (dir.join("lines.py"), dir.join("paragraphs"));
    std::fs::write(
        &script,
        QT_LINES.replacen("app =", &format!("{QT_SELECTION}\napp ="), 1),
    )
    .unwrap();
    std::fs::write(&texts, PARAGRAPHS.join("\n")).unwrap();
    let out = std::process::Command::new(PYTHON)
        .arg(&script)
        .args([family, &px.to_string()])
        .arg(&texts)
        .args([width.to_string(), flags.to_string()])
        .env("QT_QPA_PLATFORM", "offscreen")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{PYTHON} failed:\n{log}");
    let out = String::from_utf8(out.stdout).unwrap();
    let mut rows = out.lines().map(|l| {
        l.split_whitespace()
            .flat_map(|n| n.parse::<i64>().ok())
            .collect::<Vec<i64>>()
    });
    let all: Vec<Lines> = PARAGRAPHS
        .iter()
        .map(|_| {
            let n = rows.next().expect("a paragraph")[0];
            (0..n)
                .map(|_| {
                    let xs = rows.next().expect("a line");
                    let picks = rows.next().expect("a line's picks");
                    let row = rows.next().expect("a line's selections");
                    let (mut at, mut selections) = (0, vec![]);
                    while at < row.len() {
                        let n = row[at + 2] as usize;
                        let rects = row[at + 3..at + 3 + 2 * n]
                            .chunks(2)
                            .map(|r| (r[0], r[1]))
                            .collect();
                        selections.push((row[at] as usize, row[at + 1] as usize, rects));
                        at += 3 + 2 * n;
                    }
                    (
                        xs[0] as usize,
                        xs[1] as usize,
                        xs[2],
                        xs[3..].to_vec(),
                        picks.iter().map(|&p| p as usize).collect(),
                        selections,
                    )
                })
                .collect()
        })
        .collect();
    assert!(rows.next().is_none(), "Qt printed:\n{out}\n{log}");
    all
}

#[test]
fn carets_of_wrapped_lines_match_qt() {
    if !std::path::Path::new(PYTHON).exists() {
        testenv::unavailable(format_args!("`{PYTHON}` (PySide6) is not installed"));
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut cases, mut wrong) = (0, vec![]);
    let (mut selections, mut wrong_selections) = (0, vec![]);
    for family in ["DejaVu Sans", "Noto Naskh Arabic"] {
        let path = fc_match_file(family).unwrap();
        let px = 16;
        let stack = FontStack::from_path(&path, px, 400.0, &[]).unwrap();
        for (width, flags) in [
            (60, 1),
            (60, 3),
            (60, 5),
            (100, 7),
            (100, 1),
            (150, 4),
            (400, 6),
        ] {
            let qt = qt_lines(dir.path(), family, px, width, flags);
            for (text, lines) in PARAGRAPHS.iter().zip(&qt) {
                let mut map = stack.line_map(text, &|_| true);
                let chars: Vec<char> = text.chars().collect();
                for (n, (from, units, natural, qt_xs, qt_picks, qt_selections)) in
                    lines.iter().enumerate()
                {
                    cases += 1;
                    let last = n + 1 == lines.len();
                    // The white space the text ends with is text of its last line.
                    let mut trailing = chars[*from..from + units]
                        .iter()
                        .rev()
                        .take_while(|&&c| c == ' ' && !last)
                        .count();
                    let wd = width as i64 * 64;
                    // A tab that does not fit in a line with text ends it, as its white
                    // space; one that fits is text of the line (QTextLine::layout_helper).
                    if !last && *units > 1 && chars[from + units - 1] == '\t' {
                        let before = qsg::text::MapLine {
                            from: *from,
                            len: units - 1,
                            trailing: 0,
                            text_width: 0,
                            keep_spaces: false,
                        };
                        map.shape_line(&before);
                        let x = map.advance(*from, from + units - 1);
                        trailing = (x + qsg::text::tab_width(x) > wd) as usize;
                    }
                    let (keep_spaces, len) = (flags & 2 != 0, units - trailing);
                    // The width of a line is the layout's to tell: it is Qt's here.
                    let line = qsg::text::MapLine {
                        from: *from,
                        len,
                        trailing,
                        text_width: *natural,
                        keep_spaces,
                    };
                    map.shape_line(&line);
                    let left = map.align(&line, wd, if flags & 4 != 0 { 2 } else { 0 });
                    let xs: Vec<i64> = (*from..=from + units)
                        .map(|c| {
                            let x = left + map.cursor_to_x_in(&line, c);
                            // QTextLine::cursorToX keeps it in a line that wraps.
                            if flags & 1 != 0 { x.clamp(0, wd) } else { x }
                        })
                        .collect();
                    let picks: Vec<usize> = (-6..qt_picks.len() as i64 - 6)
                        .map(|h| map.x_to_cursor_in(&line, h * 32 - left, last))
                        .collect();
                    for (a, b, qt_rects) in qt_selections {
                        selections += 1;
                        let rects = joined(
                            map.ranges(&line, *a, *b)
                                .into_iter()
                                .map(|(l, r)| (left + l, left + r))
                                .collect(),
                        );
                        if &rects != qt_rects {
                            wrong_selections.push(format!(
                                "{family} width {width} flags {flags} {text:?} line {n} ({from}+{len}+{trailing}) selection {a}..{b}:\n  ours {rects:?}\n  Qt   {qt_rects:?}"
                            ));
                        }
                    }
                    if (&xs, &picks) != (qt_xs, qt_picks) {
                        wrong.push(format!(
                            "{family} width {width} flags {flags} {text:?} line {n} ({from}+{len}+{trailing}):\n  ours {xs:?}\n  Qt   {qt_xs:?}\n  ours {picks:?}\n  Qt   {qt_picks:?}"
                        ));
                    }
                }
            }
        }
    }
    elog::eprintln!(
        "{cases} lines compared with Qt, {} differ; {selections} selections, {} differ",
        wrong.len(),
        wrong_selections.len()
    );
    assert!(
        wrong.is_empty(),
        "{} of {cases} lines differ from Qt:\n{}",
        wrong.len(),
        wrong.join("\n")
    );

    assert!(
        wrong_selections.is_empty(),
        "selections of {selections} differ from Qt, the first {}:\n{}",
        wrong_selections.len(),
        wrong_selections.join("\n")
    );
}

/// The stretches from left to right, those that touch as one: Qt Quick draws one rectangle
/// behind selected glyph runs next to each other.
fn joined(mut ranges: Vec<(i64, i64)>) -> Vec<(i64, i64)> {
    ranges.retain(|r| r.0 < r.1);
    ranges.sort();
    let mut out: Vec<(i64, i64)> = vec![];
    for (l, r) in ranges {
        match out.last_mut() {
            Some(last) if l <= last.1 => last.1 = last.1.max(r),
            _ => out.push((l, r)),
        }
    }
    out
}
