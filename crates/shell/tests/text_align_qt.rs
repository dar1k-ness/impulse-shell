//! Where a Text puts its lines across its width, compared with Qt itself (Qt Quick drawing
//! through OpenGL, support/qt_gl.rs; the Texts are NativeRendering, quickshell's default):
//! text that reads right to left is aligned right unless `horizontalAlignment` is set
//! (QQuickTextPrivate::determineHorizontalAlignment), `LayoutMirroring` swaps left and right,
//! and `contentWidth` is the width of the lines where they are put, each as wide as its
//! advance plus what its last glyph reaches past it (QTextLine::naturalTextRect).
//!
//! Per case: `horizontalAlignment`, `effectiveHorizontalAlignment`, `contentWidth`,
//! `lineCount`, `implicitWidth`, and the pixels of its cell of the picture.

#[path = "support/qt_gl.rs"]
mod qt_gl;

use qsg::raster::Canvas;
use shell::scene::{self, Target};

const FAMILIES: [&str; 2] = ["Noto Sans", "Noto Naskh Arabic"];

/// Left to right, right to left, and both with either first: the first letter with a
/// direction decides (QString::isRightToLeft).
const TEXTS: [&str; 4] = [
    "AB Wij mA BWi jm ABW",
    "مرحبا بالعالم سلام كتاب",
    "12 مرحبا abc سلام de",
    "abc مرحبا de سلام كتاب",
];

/// Upright and slanted: a slanted last glyph reaches past its advance.
const STYLES: [&str; 2] = ["", "font.italic: true; "];

const LAYOUTS: [&str; 6] = [
    "",
    "width: 120; ",
    "width: 120; LayoutMirroring.enabled: true; ",
    "width: 60; wrapMode: Text.Wrap; ",
    "width: 60; wrapMode: Text.Wrap; maximumLineCount: 2; elide: Text.ElideRight; ",
    "width: 70; elide: Text.ElideRight; ",
];

const ALIGNMENTS: [&str; 4] = [
    "",
    "horizontalAlignment: Text.AlignLeft; ",
    "horizontalAlignment: Text.AlignRight; ",
    "horizontalAlignment: Text.AlignHCenter; ",
];

const PROPS: [&str; 5] = [
    "horizontalAlignment",
    "effectiveHorizontalAlignment",
    "contentWidth",
    "lineCount",
    "implicitWidth",
];

/// The signals counted for each case, after its properties.
const SIGNALS: [&str; 2] = [
    "HorizontalAlignmentChanged",
    "EffectiveHorizontalAlignmentChanged",
];

/// What is compared of a case: `PROPS`, then how often each of `SIGNALS` came.
const VALUES: usize = PROPS.len() + SIGNALS.len();

/// A case's cell of the picture.
const CELL: (usize, usize) = (130, 150);
const COLUMNS: usize = 14;
/// As many cases as one picture has: it is a window on sway's 1920x1080 output.
const PICTURE: usize = COLUMNS * 6;

fn cases() -> Vec<String> {
    let mut cases = vec![];
    for text in TEXTS {
        for style in STYLES {
            for layout in LAYOUTS {
                for alignment in ALIGNMENTS {
                    cases.push(format!(
                        "Text {{ @ {style}{alignment}{layout}text: \"{text}\" }}"
                    ));
                }
            }
        }
    }
    cases
}

fn size(cases: usize) -> (usize, usize) {
    (CELL.0 * COLUMNS, CELL.1 * cases.div_ceil(COLUMNS))
}

/// Every case's Text in its cell of a white rectangle. A case is an item with `@` where its
/// Text's id, place, colour and font go, and `$` for its number: the Text is `t$`. The root
/// has what is compared of case `i` as `p{i}_{k}`.
fn source(family: &str, cases: &[String]) -> String {
    let (width, height) = size(cases.len());
    let mut s = format!(
        "import QtQuick\nRectangle {{\n    id: root\n    width: {width}; height: {height}; \
         color: \"white\"\n"
    );
    for (i, case) in cases.iter().enumerate() {
        let size = match case.contains("font.pixelSize") {
            true => "",
            false => "font.pixelSize: 16; ",
        };
        let mut text = format!(
            "id: t{i}; x: {}; y: {}; color: \"black\"; font.family: \"{family}\"; \
             {size}renderType: Text.NativeRendering;",
            CELL.0 * (i % COLUMNS) + 4,
            CELL.1 * (i / COLUMNS) + 4
        );
        for (k, signal) in SIGNALS.iter().enumerate() {
            text += &format!(" on{signal}: root.p{i}_{} += 1;", PROPS.len() + k);
        }
        let item = case.replace('@', &text).replace('$', &i.to_string());
        s += &format!("    {item}\n");
        for (k, prop) in PROPS.iter().enumerate() {
            s += &format!("    property real p{i}_{k}: t{i}.{prop}\n");
        }
        for k in PROPS.len()..VALUES {
            s += &format!("    property real p{i}_{k}: 0\n");
        }
    }
    s
}

type Measured = Vec<Vec<f64>>;

/// What Qt has once `setup` ran, and its picture.
fn qt(source: &str, cases: usize, setup: &str) -> (Measured, Canvas) {
    let source = format!(
        "{source}    Component.onCompleted: {{\n        {setup}\n        \
         for (let i = 0; i < {cases}; i++) {{\n            \
         let s = \"CASE\";\n            \
         for (let k = 0; k < {props}; k++)\n                \
         s += \" \" + root[\"p\" + i + \"_\" + k];\n            \
         console.log(s);\n        }}\n    }}\n}}\n",
        props = VALUES
    );
    let (log, picture) = qt_gl::run(&source);
    let measured: Measured = log
        .lines()
        .filter_map(|l| l.split_once("CASE ").map(|l| l.1))
        .map(|l| l.split(' ').map(|v| v.parse().unwrap()).collect())
        .collect();
    assert_eq!(measured.len(), cases, "Qt printed:\n{log}");
    (measured, picture)
}

/// The same under our engine, with text measured when written or read, as the shell sets it up.
fn ours(source: &str, cases: usize, setup: &str) -> (Measured, Canvas) {
    let mut rt = qml::runtime::Runtime::new(None);
    let fonts = std::rc::Rc::new(std::cell::RefCell::new(scene::Fonts::default()));
    {
        let fonts = fonts.clone();
        rt.world.borrow_mut().text_measure = Some(Box::new(move |w, i| {
            if let Ok(mut f) = fonts.try_borrow_mut() {
                scene::measure_text(w, &mut f, i);
            }
        }));
    }
    let source = format!("{source}    Component.onCompleted: {{\n        {setup}\n    }}\n}}\n");
    let root = rt.load_str(&source).unwrap();
    let (width, height) = size(cases);
    rt.settle();
    scene::layout_window(
        &mut rt,
        &mut fonts.borrow_mut(),
        root,
        [width as f64, height as f64],
    );
    rt.settle();
    let measured = {
        let mut w = rt.world.borrow_mut();
        (0..cases)
            .map(|i| {
                (0..VALUES)
                    .map(|k| {
                        w.read(root, &format!("p{i}_{k}"))
                            .map_or(f64::NAN, |v| v.to_number())
                    })
                    .collect()
            })
            .collect()
    };
    let mut picture = Canvas::new(width, height, [0, 0, 0, 0]);
    scene::paint_window(
        &mut rt,
        &mut fonts.borrow_mut(),
        root,
        &mut Target::Cpu(&mut picture),
    );
    (measured, picture)
}

/// The case's cell: how many pixels differ and by how much at most in a channel.
fn cell_difference(a: &Canvas, b: &Canvas, case: usize) -> (usize, u8) {
    let (x0, y0) = (CELL.0 * (case % COLUMNS), CELL.1 * (case / COLUMNS));
    let (mut count, mut most) = (0, 0);
    for y in y0..y0 + CELL.1 {
        for x in x0..x0 + CELL.0 {
            let (a, b) = (a.px[y * a.w + x], b.px[y * b.w + x]);
            let d = (0..4).map(|k| a[k].abs_diff(b[k])).max().unwrap_or(0);
            count += (d > 0) as usize;
            most = most.max(d);
        }
    }
    (count, most)
}

/// `cases` in `family`, measured and drawn once the statements of `setup` ran.
fn compare(family: &str, cases: &[String], setup: &str) {
    if !qt_gl::available() {
        return;
    }
    assert!(setup.is_empty() || cases.len() <= PICTURE);
    let mut wrong = Vec::new();
    for cases in cases.chunks(PICTURE) {
        let source = source(family, cases);
        let ((qt, qt_picture), (ours, our_picture)) = (
            qt(&source, cases.len(), setup),
            ours(&source, cases.len(), setup),
        );
        assert_eq!((qt_picture.w, qt_picture.h), size(cases.len()));
        for (i, case) in cases.iter().enumerate() {
            let mut differs: Vec<String> = PROPS
                .iter()
                .chain(&SIGNALS)
                .zip(qt[i].iter().zip(&ours[i]))
                .filter(|(_, (qt, ours))| qt != ours)
                .map(|(name, (qt, ours))| format!("{name} {ours}, Qt has {qt}"))
                .collect();
            let (count, most) = cell_difference(&qt_picture, &our_picture, i);
            if most > 0 {
                differs.push(format!("{count} pixels differ, by up to {most} of 255"));
            }
            if !differs.is_empty() {
                let case = case.replace('$', &i.to_string());
                wrong.push(format!("{case}: {}", differs.join("; ")));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{family}: {} of {} cases differ from Qt\n{}",
        wrong.len(),
        cases.len(),
        wrong.join("\n")
    );
}

#[test]
fn noto_sans_lines_are_put_as_qt_puts_them() {
    compare(FAMILIES[0], &cases(), "");
}

#[test]
fn noto_naskh_arabic_lines_are_put_as_qt_puts_them() {
    compare(FAMILIES[1], &cases(), "");
}

const LATIN: &str = "AB Wij mA BWi";
const ARABIC: &str = "مرحبا بالعالم سلام";

/// An alignment that is set, set again and taken back (`undefined`), a text that changes
/// direction, and mirroring that is the item's own, an ancestor's, or switched on later:
/// each as (item, what is done to it once it is loaded).
fn changing() -> Vec<(String, String)> {
    let mirrored = |own: &str, text: &str| {
        format!(
            "Item {{ id: m$; LayoutMirroring.enabled: true; {own}Text {{ @ width: 120; {text} }} }}"
        )
    };
    let inherit = "LayoutMirroring.childrenInherit: true; ";
    let mut cases = vec![];
    for text in [LATIN, ARABIC] {
        let plain = format!("Text {{ @ width: 120; text: \"{text}\" }}");
        let left = format!(
            "Text {{ @ width: 120; horizontalAlignment: Text.AlignLeft; text: \"{text}\" }}"
        );
        let set = "t$.horizontalAlignment = Text.AlignLeft;";
        let reset = "t$.horizontalAlignment = undefined;";
        for (item, setup) in [
            (
                plain.clone(),
                "t$.horizontalAlignment = Text.AlignRight;".to_string(),
            ),
            (plain.clone(), set.to_string()),
            (plain.clone(), format!("{set} {reset}")),
            (left.clone(), reset.to_string()),
            (
                left.clone(),
                format!("{reset} t$.horizontalAlignment = Text.AlignHCenter;"),
            ),
            (plain.clone(), format!("t$.text = \"{LATIN}\";")),
            (plain.clone(), format!("t$.text = \"{ARABIC}\";")),
            (left.clone(), format!("t$.text = \"{ARABIC}\";")),
            (plain.clone(), "t$.text = \"\";".to_string()),
            (
                plain.clone(),
                "t$.LayoutMirroring.enabled = true;".to_string(),
            ),
            (
                left.clone(),
                "t$.LayoutMirroring.enabled = true;".to_string(),
            ),
            (
                format!(
                    "Text {{ @ width: 120; property bool flip: false; horizontalAlignment: \
                     flip ? Text.AlignRight : Text.AlignLeft; text: \"{text}\" }}"
                ),
                "t$.flip = true;".to_string(),
            ),
        ] {
            cases.push((item, setup));
        }
        // Taken back under mirroring. Text that reads left to right is in
        // `an_alignment_taken_back_under_mirroring_is_laid_out_anew`: Qt leaves it as it was.
        if text == ARABIC {
            cases.push((
                left.clone(),
                format!("t$.LayoutMirroring.enabled = true; {reset}"),
            ));
        }
        for alignment in ALIGNMENTS {
            let text = format!("{alignment}text: \"{text}\"");
            cases.push((mirrored("", &text), String::new()));
            cases.push((mirrored(inherit, &text), String::new()));
            cases.push((
                mirrored(inherit, &format!("LayoutMirroring.enabled: false; {text}")),
                String::new(),
            ));
            cases.push((
                mirrored(inherit, &text),
                "m$.LayoutMirroring.enabled = false;".to_string(),
            ));
            cases.push((
                mirrored("", &text),
                "m$.LayoutMirroring.childrenInherit = true;".to_string(),
            ));
        }
        // Text with tags follows the same rules; wrapped, a line ends where a tag does.
        let (first, rest) = text.split_once(' ').unwrap();
        for alignment in ALIGNMENTS {
            for (layout, text) in [
                ("width: 120; ", format!("<b>{text}</b>")),
                (
                    "width: 60; wrapMode: Text.Wrap; ",
                    format!("<b>{first}</b> {rest}"),
                ),
            ] {
                cases.push((
                    format!(
                        "Text {{ @ textFormat: Text.StyledText; {alignment}{layout}text: \
                         \"{text}\" }}"
                    ),
                    String::new(),
                ));
            }
        }
    }
    cases
}

#[test]
fn alignment_follows_what_is_set_later_as_in_qt() {
    let (cases, setup): (Vec<String>, Vec<String>) = changing().into_iter().unzip();
    let setup: Vec<String> = setup
        .iter()
        .enumerate()
        .map(|(i, s)| s.replace('$', &i.to_string()))
        .collect();
    compare(FAMILIES[0], &cases, &setup.join("\n        "));
}

/// A Text whose alignment is set from outside, through an alias of the item around it
/// (`property alias horizontalAlignment: t.horizontalAlignment`, Waffle's
/// WTextWithFixedWidth): the alias has the Text's own setter, so the alignment is a set one
/// and no longer follows the direction of the text. Declared, bound, written and taken back
/// by a script, through an alias of the alias, and the two Texts of Waffle that are centred
/// so: the minutes of the focus timer and the number of the OSD.
fn through_alias() -> Vec<(String, String)> {
    let alias = "property alias horizontalAlignment: t$.horizontalAlignment; ";
    let around = |own: &str, text: &str| {
        format!("Item {{ id: a$; {alias}{own}Text {{ @ width: 120; text: \"{text}\" }} }}")
    };
    let set = |to: &str| format!("a$.horizontalAlignment = Text.{to};");
    let reset = "a$.horizontalAlignment = undefined;";
    let either = "property bool flip: false; \
                  horizontalAlignment: flip ? Text.AlignRight : Text.AlignHCenter; ";
    let mut cases = vec![];
    for text in [LATIN, ARABIC] {
        for alignment in &ALIGNMENTS[1..] {
            cases.push((around(alignment, text), String::new()));
        }
        cases.push((around(either, text), String::new()));
        cases.push((around(either, text), "a$.flip = true;".to_string()));
        cases.push((around("", text), set("AlignHCenter")));
        cases.push((around("", text), set("AlignLeft")));
        cases.push((around(ALIGNMENTS[3], text), set("AlignRight")));
        cases.push((around(ALIGNMENTS[3], text), reset.to_string()));
        cases.push((around("", text), format!("{} {reset}", set("AlignHCenter"))));
        cases.push((
            around(
                &format!("LayoutMirroring.enabled: true; {}", ALIGNMENTS[1]),
                text,
            ),
            String::new(),
        ));
        cases.push((
            format!(
                "Item {{ id: b$; property alias horizontalAlignment: a$.horizontalAlignment; \
                 {}{} }}",
                ALIGNMENTS[3],
                around("", text)
            ),
            String::new(),
        ));
    }
    for (width, text) in [
        (81, "<font color=\\\"#ff0000\\\">25</font> mins"),
        (16, "45"),
    ] {
        cases.push((
            format!(
                "Item {{ id: a$; {alias}{}Text {{ @ width: {width}; text: \"{text}\" }} }}",
                ALIGNMENTS[3]
            ),
            String::new(),
        ));
    }
    cases
}

#[test]
fn an_alignment_set_through_an_alias_is_the_texts_own_as_in_qt() {
    let (cases, setup): (Vec<String>, Vec<String>) = through_alias().into_iter().unzip();
    let setup: Vec<String> = setup
        .iter()
        .enumerate()
        .map(|(i, s)| s.replace('$', &i.to_string()))
        .collect();
    compare(FAMILIES[0], &cases, &setup.join("\n        "));
}

/// Text with a tag over part of a line that is not wrapped: bold, slanted, coloured,
/// underlined, at the start, in the middle and at the end of text that reads right to left,
/// and over the part of mixed text that reads the other way. Qt lays the line out once and
/// the tag only says how its letters are drawn (QTextLine over QTextLayout::formats), so the
/// tagged part is where the line's order puts it.
fn tagged() -> Vec<String> {
    let words: Vec<&str> = TEXTS[1].split(' ').collect();
    let mut texts = vec![];
    for (open, close) in [
        ("<b>", "</b>"),
        ("<i>", "</i>"),
        ("<u>", "</u>"),
        ("<font color='red'>", "</font>"),
    ] {
        for (from, to) in [(0, 1), (1, 2), (1, 3), (3, 4)] {
            let (before, inside, after) = (
                words[..from].join(" "),
                words[from..to].join(" "),
                words[to..].join(" "),
            );
            texts.push(
                format!("{before} {open}{inside}{close} {after}")
                    .trim()
                    .to_string(),
            );
        }
    }
    // Half a word, and two tags on one line.
    texts.push("مر<b>حبا</b> بالعالم سلام".to_string());
    texts.push("<b>مرحبا</b> بالعالم <i>سلام</i> كتاب".to_string());
    // Mixed: the tag over the part that reads the other way, and over both.
    texts.push("abc <b>مرحبا</b> de سلام".to_string());
    texts.push("abc مر<b>حبا de</b> سلام".to_string());
    texts.push("مرحبا <b>abc</b> سلام de".to_string());
    texts.push("12 <b>مرحبا abc</b> سلام".to_string());
    let mut cases = vec![];
    for text in texts {
        let text = text.replace('\'', "\\\"");
        for layout in ["", "width: 125; "] {
            for alignment in [ALIGNMENTS[0], ALIGNMENTS[1]] {
                cases.push(format!(
                    "Text {{ @ textFormat: Text.StyledText; {alignment}{layout}text: \"{text}\" }}"
                ));
            }
        }
    }
    cases
}

#[test]
fn a_tag_over_part_of_a_line_is_ordered_as_in_qt() {
    for family in FAMILIES {
        compare(family, &tagged(), "");
    }
}

/// Underlined text in four sizes: at the start and inside a line, over text that reads either
/// way and both ways, with a colour or a weight changing under the line and in two tags side
/// by side. Qt draws one line under neighbours of one colour, with the metrics of the font
/// whose line is the thickest, as a stroke whose edges are soft (QQuickTextNodeEngine::
/// processCurrentLine, QSGRhiInternalTextNode::addDecorationNode).
fn underlined() -> Vec<String> {
    let mut cases = vec![];
    for text in [
        "<u>AB W</u>ij",
        "A<u>B W</u>ij",
        "<u>- - - -</u>",
        "<u>سلام</u> كتاب",
        "<u>ab سلام</u> c",
        "<u>A<font color='red'>B W</font>i</u>j",
        "<u>AB</u><u> W</u>ij",
        "<u>A<b>B W</b></u>ij",
    ] {
        let text = text.replace('\'', "\\\"");
        for size in [12, 16, 20, 28] {
            cases.push(format!(
                "Text {{ @ font.pixelSize: {size}; textFormat: Text.StyledText; \
                 text: \"{text}\" }}"
            ));
        }
    }
    cases
}

#[test]
fn an_underline_is_drawn_as_in_qt() {
    for family in FAMILIES {
        compare(family, &underlined(), "");
    }
}

/// docs/qt-differences.md: an AlignLeft set on text that reads left to right, mirrored, then
/// taken back. Qt reads AlignLeft for both properties, as here, but sends no signal and lays
/// nothing out (QQuickTextPrivate::setHAlign has AlignLeft before and after): a binding to
/// `effectiveHorizontalAlignment` keeps AlignRight and the text stays at the right. Here the
/// signal is sent, the binding follows and the text is where a Text that was never given an
/// alignment has it. The third case holds what the script read.
#[test]
fn an_alignment_taken_back_under_mirroring_is_laid_out_anew() {
    if !qt_gl::available() {
        return;
    }
    let cases = [
        format!("Text {{ @ width: 120; horizontalAlignment: Text.AlignLeft; text: \"{LATIN}\" }}"),
        format!("Text {{ @ width: 120; LayoutMirroring.enabled: true; text: \"{LATIN}\" }}"),
    ];
    let read: String = (0..VALUES)
        .map(|k| format!("    property real p2_{k}: 0\n"))
        .collect();
    let source = source(FAMILIES[0], &cases) + &read;
    let setup = "t0.LayoutMirroring.enabled = true; t0.horizontalAlignment = undefined; \
                 root.p2_0 = t0.horizontalAlignment; root.p2_1 = t0.effectiveHorizontalAlignment;";
    let ((qt, qt_picture), (ours, our_picture)) = (qt(&source, 3, setup), ours(&source, 3, setup));
    let (signals, effective) = (PROPS.len(), PROPS.len() + 1);
    // Read by the script, and the horizontalAlignmentChanged that never comes: the same.
    assert_eq!(qt[2][..2], [1.0, 1.0], "Qt reads AlignLeft for both");
    assert_eq!(ours[2][..2], [1.0, 1.0], "both read AlignLeft here");
    assert_eq!(
        (qt[0][signals], ours[0][signals]),
        (0.0, 0.0),
        "horizontalAlignmentChanged"
    );
    // Qt: one effectiveHorizontalAlignmentChanged, for the mirroring; the binding and the
    // text keep AlignRight.
    assert_eq!(
        qt[0][effective], 1.0,
        "Qt's effectiveHorizontalAlignmentChanged"
    );
    assert_eq!(
        qt[0][1], 2.0,
        "Qt's binding to effectiveHorizontalAlignment"
    );
    let moved = |picture: &Canvas| {
        (0..CELL.1).any(|y| {
            (0..CELL.0)
                .any(|x| picture.px[y * picture.w + x] != picture.px[y * picture.w + x + CELL.0])
        })
    };
    assert!(moved(&qt_picture), "Qt draws the text where it was");
    // Here: a second signal, the binding follows, and the text is at the left as the
    // mirrored Text beside it, which is as Qt has that one.
    assert_eq!(
        ours[0][effective], 2.0,
        "effectiveHorizontalAlignmentChanged here"
    );
    assert_eq!(
        ours[0][..signals],
        ours[1][..signals],
        "the binding follows here"
    );
    assert_eq!(ours[1], qt[1], "a Text never given an alignment");
    assert!(!moved(&our_picture), "the text is laid out anew here");
    assert_eq!(cell_difference(&qt_picture, &our_picture, 1), (0, 0));
}
