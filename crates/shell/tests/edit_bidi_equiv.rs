//! The caret and the hit test of a TextInput and a TextEdit whose text is set right to left,
//! or mixes both directions, compared with Qt itself (`qml6`, offscreen) on the fonts of the
//! machine the test runs on: `positionToRectangle` of every position and `positionAt` of
//! points along every line, for each alignment, wrapped and not.
//! Ours measured the text before the caret on its own and aligned every line to the left: in
//! "שלום עולם" the caret before the first letter stood at the left end of the line, Qt has it
//! at the right end, of a line it sets against the right edge of the editor.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

/// As QML string literals: one direction; both, starting with either; digits and brackets in
/// right-to-left text; Arabic, whose letters join; white space at the ends; lines of a
/// TextEdit of different directions; tabs, each as wide as the way to the next tab stop.
const TEXTS: [&str; 18] = [
    "plain latin text",
    "שלום עולם",
    "abc שלום def",
    "שלום abc def עולם",
    "abc def שלום עולם ghi jkl",
    "שלום 123 (עולם) 45.6",
    "مرحبا بالعالم",
    "العربية mixed with English هنا",
    "  שלום  ",
    "abc  ",
    "שלום עולם, זה טקסט ארוך למדי שנשבר",
    "one two three four five six seven",
    "שלום abc\\nlatin line\\nעברית",
    "latin first\\nשלום עולם\\n\\nend",
    "a\\tb",
    "\\tif x:\\treturn y",
    "ab\\t\\tcd ef\\t",
    "שלום\\tabc",
];

const PROPS: [&str; 9] = [
    "",
    "horizontalAlignment: TextInput.AlignLeft",
    "horizontalAlignment: TextInput.AlignRight",
    "horizontalAlignment: TextInput.AlignHCenter",
    "wrapMode: TextInput.WordWrap; width: 60",
    "wrapMode: TextInput.WordWrap; width: 100",
    "wrapMode: TextInput.WordWrap; width: 100; horizontalAlignment: TextInput.AlignLeft",
    "wrapMode: TextInput.WordWrap; width: 100; horizontalAlignment: TextInput.AlignHCenter",
    "leftPadding: 7; rightPadding: 11; topPadding: 3",
];

const FONT: &str = "font.family: \"DejaVu Sans\"; font.pixelSize: 16";

/// (the editor's type, its properties)
fn cases() -> Vec<(&'static str, String)> {
    let mut cases = Vec::new();
    for kind in ["TextInput", "TextEdit"] {
        for text in TEXTS {
            if kind == "TextInput" && text.contains("\\n") {
                continue;
            }
            for props in PROPS {
                let width = if props.contains("width") {
                    ""
                } else {
                    "width: 240; "
                };
                let sep = if props.is_empty() { "" } else { "; " };
                // A TextInput scrolls to its cursor, which this does not compare.
                let scroll = if kind == "TextInput" {
                    "autoScroll: false; "
                } else {
                    ""
                };
                cases.push((kind, format!("{scroll}{width}{props}{sep}text: \"{text}\"")));
            }
        }
    }
    cases
}

/// Per case an editor; Qt prints `positionToRectangle` (x, y) of every position, in the
/// editor's coordinates (QQuickTextInput::positionToRectangle leaves the padding out), then the
/// points of the middle of every line, 3 px apart from 6 px left of the editor to 6 px right
/// of it, with their `positionAt`.
fn source(qt: bool) -> String {
    let mut s = String::from("import QtQuick\nItem {\n    id: root\n");
    let cases = cases();
    for (i, (kind, props)) in cases.iter().enumerate() {
        s += &format!("    {kind} {{ id: e{i}; {FONT}; {props} }}\n");
        s += &format!("    property Item item{i}: e{i}\n");
    }
    if qt {
        s += &format!(
            "    Component.onCompleted: {{\n        for (let i = 0; i < {}; i++) {{\n            \
             const e = root[\"item\" + i];\n            const rects = [], hits = [], ys = [];\n            \
             const input = e instanceof TextInput;\n            \
             for (let p = 0; p <= e.length; p++) {{\n                \
             const r = e.positionToRectangle(p);\n                \
             const y = r.y + (input ? e.topPadding : 0);\n                \
             rects.push(r.x + (input ? e.leftPadding : 0), y);\n                \
             if (ys.indexOf(y + r.height / 2) < 0)\n                    \
             ys.push(y + r.height / 2);\n            }}\n            \
             for (const y of ys)\n                \
             for (let x = -6; x <= e.width + 6; x += 3)\n                    \
             hits.push(x, y, e.positionAt(x, y));\n            \
             console.log(\"CASE\", i, e.horizontalAlignment, rects.join(\" \"), \"|\", \
             hits.join(\" \"));\n        }}\n        Qt.quit();\n    }}\n",
            cases.len()
        );
    }
    s += "}\n";
    s
}

/// `horizontalAlignment`, (x, y) of the caret at every position, and (x, y, position) of the
/// points hit.
type Laid = (f64, Vec<f64>, Vec<f64>);

fn numbers(s: &str) -> Vec<f64> {
    s.split_whitespace().map(|n| n.parse().unwrap()).collect()
}

fn qt_laid() -> Vec<Laid> {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("edit.qml");
    std::fs::write(&file, source(true)).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&file)
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    let laid: Vec<Laid> = log
        .lines()
        .filter_map(|l| l.split_once("CASE ")?.1.split_once(' ')?.1.split_once('|'))
        .map(|(rects, hits)| {
            let rects = numbers(rects);
            (rects[0], rects[1..].to_vec(), numbers(hits))
        })
        .collect();
    assert_eq!(laid.len(), cases().len(), "qml6 printed:\n{log}");
    laid
}

/// Ours at the points Qt was asked about.
fn our_laid(qt: &[Laid]) -> Vec<Laid> {
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
    let root = rt.load_str(&source(false)).unwrap();
    rt.settle();
    scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, [400.0, 300.0]);
    rt.settle();
    let mut w = rt.world.borrow_mut();
    let mut fonts = fonts.borrow_mut();
    qt.iter()
        .enumerate()
        .map(|(i, (_, _, hits))| {
            let Some(Value::Item(e)) = w.read(root, &format!("item{i}")) else {
                panic!("item{i} is not an item");
            };
            let align = w
                .read(e, "horizontalAlignment")
                .map_or(f64::NAN, |v| v.to_number());
            let lines = scene::edit_geometry(&mut w, &mut fonts, e);
            let end = lines.last().map_or(0, |l| l.0 + l.3.len() - 1);
            let rects = (0..=end)
                .flat_map(|p| {
                    let (start, y, _, xs) = lines.iter().rev().find(|l| l.0 <= p).unwrap();
                    [xs[(p - start).min(xs.len() - 1)], *y]
                })
                .collect();
            let hits = hits
                .chunks(3)
                .flat_map(|h| {
                    let at = scene::edit_position_at(&mut w, &mut fonts, e, h[0], h[1]);
                    [h[0], h[1], at.map_or(f64::NAN, |p| p as f64)]
                })
                .collect();
            (align, rects, hits)
        })
        .collect()
}

fn same(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1.0 / 64.0)
}

fn check(kind: &str) {
    if !testenv::require("qml6") {
        return;
    }
    let qt = qt_laid();
    let ours = our_laid(&qt);
    let cases = cases();
    let checked: Vec<_> = cases
        .iter()
        .zip(qt.iter().zip(&ours))
        .filter(|(case, _)| case.0 == kind)
        .collect();
    let wrong: Vec<String> = checked
        .iter()
        .filter(|(_, (qt, ours))| qt.0 != ours.0 || !same(&qt.1, &ours.1) || !same(&qt.2, &ours.2))
        .map(|((kind, props), (qt, ours))| {
            let hit = |l: &Laid| -> Vec<f64> { l.2.chunks(3).map(|h| h[2]).collect() };
            format!(
                "{kind} {{ {props} }}\n    ours align {} carets {:?}\n    Qt   align {} carets \
                 {:?}\n    ours hits {:?}\n    Qt   hits {:?}",
                ours.0,
                ours.1,
                qt.0,
                qt.1,
                hit(ours),
                hit(qt)
            )
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "{} of {} cases differ from Qt (horizontalAlignment; x and y of the caret at every \
         position; the position at points of every line):\n{}",
        wrong.len(),
        checked.len(),
        wrong.join("\n")
    );
}

#[test]
fn text_input_carets_match_qt() {
    check("TextInput");
}

#[test]
fn text_edit_carets_match_qt() {
    check("TextEdit");
}
