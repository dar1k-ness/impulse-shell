//! Text is measured and drawn as Qt 6.12 measures and draws it, in a bitmap-only font (one
//! 16 px strike, as a BDF, a PCF and an OpenType file: qsg/tests/fixtures/bitmap) and in
//! ordinary ones. Ours gave a bitmap font's glyphs no advance (HarfBuzz reads the advances of
//! an outline font's tables), found no glyph in a BDF or PCF file (they have no OpenType
//! `cmap`), and drew nothing at a size other than the strike's; Qt takes the strike nearest to
//! the size asked for, at every size, draws its bitmaps whatever `renderType` says, and makes
//! no bold or slanted ones of them. In an ordinary font `font.bold`, `font.italic` and
//! `font.letterSpacing` did nothing, and TextMetrics had no `boundingRect`, a `height` in whole
//! pixels and a `width` that took a line of two scripts or fonts for one run of glyphs.
//!
//! Compared with Qt itself: 13 properties of every case exactly, and the picture pixel by
//! pixel as Qt Quick draws it through OpenGL (support/qt_gl.rs; a Text that sets no
//! `renderType` is NativeRendering, quickshell's default). `Text.QtRendering` in an outline
//! font is distance fields in both, to the last bit: ours are sampled as llvmpipe samples
//! Qt's (bytes mixed at 1/256 of a texel, `float`s in the shaders' order) from a texture
//! laid out as Qt's glyph cache lays its own out, which is why Qt runs under a fixed hash
//! seed (support/qt_gl.rs). A colour glyph scaled down from its strike is compared like any
//! other: QImage's scaler is ours to the last bit. Where the lines break and are cut off is
//! read from `qml6` on the offscreen platform: values, no pixels.

#[path = "support/qt_gl.rs"]
mod qt_gl;

use qsg::raster::Canvas;
use shell::scene::{self, Target};

/// (text, the Text's other properties)
const CASES: [(&str, &str); 28] = [
    ("ABWijm", "font.pixelSize: 16"),
    ("A B  W", "font.pixelSize: 16"),
    ("ij Bm A", "font.pixelSize: 16"),
    // Sizes the bitmap font has no strike of.
    ("ABWijm", "font.pixelSize: 12"),
    ("ABWijm", "font.pixelSize: 20"),
    ("ABWijm", "font.pixelSize: 32"),
    ("ABWijm", "font.pixelSize: 16; font.bold: true"),
    ("ABWijm", "font.pixelSize: 16; font.italic: true"),
    (
        QT_RENDERING,
        "font.pixelSize: 16; renderType: Text.QtRendering",
    ),
    (
        "ABWijm",
        "font.pixelSize: 16; renderType: Text.NativeRendering",
    ),
    // Letters the bitmap font lacks, among ones it has.
    ("AxжB", "font.pixelSize: 16"),
    (
        "A B W i j m",
        "font.pixelSize: 16; width: 40; wrapMode: Text.WordWrap",
    ),
    ("ABWijm", "font.pixelSize: 16; color: \"#c00000\""),
    ("ABWijm", "font.pixelSize: 16; font.letterSpacing: 2"),
    ("A", "font.pixelSize: 16"),
    ("B", "font.pixelSize: 16"),
    ("W", "font.pixelSize: 16"),
    ("i", "font.pixelSize: 16"),
    ("j", "font.pixelSize: 16"),
    ("m", "font.pixelSize: 16"),
    ("A", "font.pixelSize: 12"),
    ("A", "font.pixelSize: 32"),
    ("W", "font.pixelSize: 16; color: \"#c00000\""),
    (COLOUR_GLYPH, "font.pixelSize: 16"),
    // An emoji alone: the line is as high as the emoji font, not as the Text's own.
    ("😀", "font.pixelSize: 19"),
    ("😀 😀", "font.pixelSize: 19"),
    // Neighbouring spans are items of their own: no kerning between them.
    (
        "mo<u>r</u><u>o</u>ck <u>T</u><u>o</u> <u>A</u><u>V</u>A",
        "font.pixelSize: 16; textFormat: Text.StyledText",
    ),
    ("Файл настроек", "font.pixelSize: 14; font.weight: 450"),
];
const QT_RENDERING: &str = "ABWijm";
const COLOUR_GLYPH: &str = "A😀B ⌘";

/// (name, what it is of case `#`'s Text `t#` and TextMetrics `m#`)
const PROPS: [(&str, &str); 13] = [
    ("implicitWidth", "t#.implicitWidth"),
    ("implicitHeight", "t#.implicitHeight"),
    ("contentWidth", "t#.contentWidth"),
    ("contentHeight", "t#.contentHeight"),
    ("baselineOffset", "t#.baselineOffset"),
    ("lineCount", "t#.lineCount"),
    ("advanceWidth", "m#.advanceWidth"),
    ("TextMetrics.width", "m#.width"),
    ("TextMetrics.height", "m#.height"),
    ("boundingRect.x", "m#.boundingRect.x"),
    ("boundingRect.y", "m#.boundingRect.y"),
    ("boundingRect.width", "m#.boundingRect.width"),
    ("boundingRect.height", "m#.boundingRect.height"),
];

/// A case's band of the picture.
const BAND: usize = 44;
/// A column of cases: the picture is a window on sway's 1920x1080 output.
const ROWS: usize = 13;
const COLUMN: usize = 360;
const SIZE: (usize, usize) = (COLUMN * CASES.len().div_ceil(ROWS), BAND * ROWS + 8);

/// Every case's Text in its band of a white rectangle, with its TextMetrics.
fn source(family: &str) -> String {
    let mut s = format!(
        "import QtQuick\nRectangle {{\n    id: root\n    width: {}; height: {}; color: \"white\"\n",
        SIZE.0, SIZE.1
    );
    for (i, (text, rest)) in CASES.into_iter().enumerate() {
        let colour = if rest.contains("color:") {
            ""
        } else {
            "; color: \"black\""
        };
        // quickshell's default; Qt Quick's own is QtRendering.
        let render = if rest.contains("renderType:") {
            ""
        } else {
            "; renderType: Text.NativeRendering"
        };
        s += &format!(
            "    Text {{ id: t{i}; x: {}; y: {}; text: \"{text}\"; font.family: \"{family}\"; \
             {rest}{colour}{render} }}\n    \
             TextMetrics {{ id: m{i}; font: t{i}.font; text: t{i}.text }}\n",
            8 + COLUMN * (i / ROWS),
            8 + BAND * (i % ROWS)
        );
        for (k, (_, of)) in PROPS.into_iter().enumerate() {
            let of = of.replace('#', &i.to_string());
            s += &format!("    property real p{i}_{k}: {of}\n");
        }
    }
    s
}

type Measured = Vec<[f64; PROPS.len()]>;

/// The properties of every case and the picture under Qt.
fn qt(source: &str) -> (Measured, Canvas) {
    let source = format!(
        "{source}    Component.onCompleted: {{\n        \
         for (let i = 0; i < {cases}; i++) {{\n            \
         let s = \"CASE\";\n            \
         for (let k = 0; k < {props}; k++)\n                \
         s += \" \" + root[\"p\" + i + \"_\" + k];\n            \
         console.log(s);\n        }}\n    }}\n}}\n",
        cases = CASES.len(),
        props = PROPS.len(),
    );
    let (log, picture) = qt_gl::run(&source);
    let measured: Measured = log
        .lines()
        .filter_map(|l| l.split_once("CASE ").map(|l| l.1))
        .map(|l| {
            let v: Vec<f64> = l.split(' ').map(|v| v.parse().unwrap()).collect();
            v.try_into().unwrap()
        })
        .collect();
    assert_eq!(measured.len(), CASES.len(), "Qt printed:\n{log}");
    (measured, picture)
}

/// The same under our engine, with text measured when written or read, as the shell sets it up.
fn ours(source: &str) -> (Measured, Canvas) {
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
    let root = rt.load_str(&format!("{source}}}\n")).unwrap();
    let size = [SIZE.0 as f64, SIZE.1 as f64];
    rt.settle();
    scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, size);
    rt.settle();
    let measured = {
        let mut w = rt.world.borrow_mut();
        (0..CASES.len())
            .map(|i| {
                std::array::from_fn(|k| {
                    w.read(root, &format!("p{i}_{k}"))
                        .map_or(f64::NAN, |v| v.to_number())
                })
            })
            .collect()
    };
    let mut picture = Canvas::new(SIZE.0, SIZE.1, [0, 0, 0, 0]);
    scene::paint_window(
        &mut rt,
        &mut fonts.borrow_mut(),
        root,
        &mut Target::Cpu(&mut picture),
    );
    (measured, picture)
}

/// The case's band: how many pixels differ and by how much at most in a channel.
fn band_difference(a: &Canvas, b: &Canvas, case: usize) -> (usize, u8) {
    let (x0, y0) = (COLUMN * (case / ROWS), BAND * (case % ROWS));
    let (mut count, mut most) = (0, 0);
    for y in y0..y0 + BAND {
        for x in x0..x0 + COLUMN {
            let (a, b) = (a.px[y * SIZE.0 + x], b.px[y * SIZE.0 + x]);
            let d = (0..4).map(|k| a[k].abs_diff(b[k])).max().unwrap_or(0);
            count += (d > 0) as usize;
            most = most.max(d);
        }
    }
    (count, most)
}

/// Every case in `family` has Qt's properties and Qt's pixels. `first` is what a bitmap
/// font's first case measures in Qt, (implicitWidth, implicitHeight, baselineOffset): the
/// family asked for is the one compared, not what fontconfig gives in its place.
fn compare(family: &str, first: Option<(f64, f64, f64)>) {
    let source = source(family);
    let ((qt, qt_picture), (ours, our_picture)) = (qt(&source), ours(&source));
    if let Some(first) = first {
        assert_eq!(
            (qt[0][0], qt[0][1], qt[0][4]),
            first,
            "Qt did not take {family}"
        );
    }
    assert_eq!((qt_picture.w, qt_picture.h), SIZE);
    let mut wrong = Vec::new();
    for (i, (text, rest)) in CASES.into_iter().enumerate() {
        let mut differs: Vec<String> = PROPS
            .iter()
            .zip(qt[i].iter().zip(&ours[i]))
            .filter(|(_, (qt, ours))| qt != ours)
            .map(|((name, _), (qt, ours))| format!("{name} {ours}, Qt has {qt}"))
            .collect();
        let (count, most) = band_difference(&qt_picture, &our_picture, i);
        if most > 0 {
            differs.push(format!("{count} pixels differ, by up to {most} of 255"));
        }
        if !differs.is_empty() {
            wrong.push(format!(
                "case {i} ({text:?}; {rest}): {}",
                differs.join("; ")
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{family}: {} of {} cases differ from Qt\n{}",
        wrong.len(),
        CASES.len(),
        wrong.join("\n")
    );
    compare_lines(family);
}

/// The font settings the lines of a Text are broken and cut off under.
const STYLES: [&str; 5] = [
    "font.pixelSize: 16",
    "font.pixelSize: 16; font.bold: true",
    "font.pixelSize: 16; font.italic: true",
    "font.pixelSize: 16; font.letterSpacing: 2",
    // A size the bitmap font has no strike of.
    "font.pixelSize: 20",
];

/// (how the Text breaks or cuts its lines, whether every line's width is compared too: Qt
/// reports the lines of text it does not cut off)
const LINES: [(&str, bool); 8] = [
    ("width: 60; wrapMode: Text.Wrap", true),
    ("width: 30; wrapMode: Text.Wrap", true),
    ("width: 60; wrapMode: Text.WordWrap", true),
    ("width: 45; wrapMode: Text.WrapAnywhere", true),
    ("width: 70; elide: Text.ElideRight", false),
    ("width: 70; elide: Text.ElideLeft", false),
    ("width: 70; elide: Text.ElideMiddle", false),
    (
        "width: 60; wrapMode: Text.Wrap; maximumLineCount: 2; elide: Text.ElideRight",
        false,
    ),
];
const WRAPPED: &str = "AB Wij mA BWi jm ABW ijm AiB";

/// What is compared of every wrapped or elided Text.
const LINE_PROPS: [&str; 6] = [
    "lineCount",
    "contentWidth",
    "contentHeight",
    "implicitWidth",
    "implicitHeight",
    "truncated",
];

/// Every style with every way of breaking: a Text, and for Qt a twin whose `lineLaidOut`
/// reports every line's width. What is compared is bound from the start, in Qt as here.
fn lines_source(family: &str, text: &str, hows: &[(&str, bool)], qt: bool) -> String {
    let mut s = String::from("import QtQuick\nItem {\n    id: root\n    property var rows: []\n");
    s += "    function laid(i, line) {\n        if (line.number === 0)\n            \
          rows[i] = [];\n        rows[i].push(line.implicitWidth);\n    }\n";
    let mut i = 0;
    for style in STYLES {
        for &(how, lines) in hows {
            let text = format!("font.family: \"{family}\"; {style}; {how}; text: \"{text}\"");
            s += &format!("    Text {{ id: t{i}; {text} }}\n");
            if qt && lines {
                s += &format!(
                    "    Text {{ {text}; onLineLaidOut: line => root.laid({i}, line) }}\n"
                );
            }
            if !qt {
                s += &format!("    property Item item{i}: t{i}\n");
            }
            for (k, prop) in LINE_PROPS.iter().enumerate() {
                s += &format!("    property real v{i}_{k}: t{i}.{prop}\n");
            }
            i += 1;
        }
    }
    if qt {
        s += &format!(
            "    Component.onCompleted: {{\n        for (let i = 0; i < {i}; i++)\n            \
             console.log(\"LINES\", i, [0, 1, 2, 3, 4, 5].map(k => root[\"v\" + i + \"_\" + k])\
             .join(\" \") + \" |\" + (rows[i] ? \" \" + rows[i].join(\" \") : \"\"));\n        \
             Qt.quit();\n    }}\n"
        );
    }
    s += "}\n";
    s
}

/// (`LINE_PROPS`, the width of every line)
type Laid = (Vec<f64>, Vec<f64>);

fn qt_lines(family: &str, text: &str, hows: &[(&str, bool)]) -> Vec<Laid> {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("lines.qml");
    std::fs::write(&file, lines_source(family, text, hows, true)).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&file)
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    let numbers =
        |s: &str| -> Vec<f64> { s.split_whitespace().map(|n| n.parse().unwrap()).collect() };
    let laid: Vec<Laid> = log
        .lines()
        .filter_map(|l| l.split_once("LINES ")?.1.split_once(' ')?.1.split_once('|'))
        .map(|(props, lines)| (numbers(props), numbers(lines)))
        .collect();
    assert_eq!(
        laid.len(),
        STYLES.len() * hows.len(),
        "qml6 printed:\n{log}"
    );
    laid
}

fn our_lines(family: &str, text: &str, hows: &[(&str, bool)]) -> Vec<Laid> {
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
    let root = rt
        .load_str(&lines_source(family, text, hows, false))
        .unwrap();
    rt.settle();
    scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, [400.0, 300.0]);
    rt.settle();
    let mut w = rt.world.borrow_mut();
    (0..STYLES.len() * hows.len())
        .map(|i| {
            let props = (0..LINE_PROPS.len())
                .map(|k| {
                    w.read(root, &format!("v{i}_{k}"))
                        .map_or(f64::NAN, |v| v.to_number())
                })
                .collect();
            let widths = match w.read(root, &format!("item{i}")) {
                Some(qml::value::Value::Item(t)) if hows[i % hows.len()].1 => {
                    scene::text_line_widths(&mut w, &mut fonts.borrow_mut(), t)
                }
                _ => vec![],
            };
            (props, widths)
        })
        .collect()
}

/// A Text in `family` that wraps or elides has Qt's lines in every style: bold, italic,
/// letter spacing and a size between the strikes reach where the lines break and what is
/// cut off.
fn compare_lines(family: &str) {
    let wrong = lines_differing(family, WRAPPED, &LINES);
    assert!(
        wrong.is_empty(),
        "{family}: {} of {} wrapped or elided cases differ from Qt (lineCount, contentWidth, \
         contentHeight, implicitWidth, implicitHeight, truncated; line widths):\n{}",
        wrong.len(),
        STYLES.len() * LINES.len(),
        wrong.join("\n")
    );
}

/// The cases of `text` in `family`, every style with every one of `hows`, that are not Qt's.
fn lines_differing(family: &str, text: &str, hows: &[(&str, bool)]) -> Vec<String> {
    let (qt, ours) = (qt_lines(family, text, hows), our_lines(family, text, hows));
    qt.iter()
        .zip(&ours)
        .enumerate()
        .filter(|(_, (qt, ours))| qt != ours)
        .map(|(i, (qt, ours))| {
            format!(
                "Text {{ font.family: {family:?}; {}; {}; text: {text:?} }}\n    ours {ours:?}\n    \
                 Qt   {qt:?}",
                STYLES[i / hows.len()],
                hows[i % hows.len()].0
            )
        })
        .collect()
}

/// Every way a Text cuts a line off, from a width the three dots alone do not fit.
const ELIDED: [(&str, bool); 17] = [
    ("width: 12; elide: Text.ElideRight", false),
    ("width: 12; elide: Text.ElideLeft", false),
    ("width: 12; elide: Text.ElideMiddle", false),
    ("width: 40; elide: Text.ElideRight", false),
    ("width: 40; elide: Text.ElideLeft", false),
    ("width: 40; elide: Text.ElideMiddle", false),
    ("width: 70; elide: Text.ElideRight", false),
    ("width: 70; elide: Text.ElideLeft", false),
    ("width: 70; elide: Text.ElideMiddle", false),
    ("width: 100; elide: Text.ElideRight", false),
    ("width: 100; elide: Text.ElideLeft", false),
    ("width: 100; elide: Text.ElideMiddle", false),
    (
        "width: 60; wrapMode: Text.Wrap; maximumLineCount: 2; elide: Text.ElideRight",
        false,
    ),
    (
        "width: 70; textFormat: Text.StyledText; elide: Text.ElideRight",
        false,
    ),
    (
        "width: 70; textFormat: Text.StyledText; elide: Text.ElideLeft",
        false,
    ),
    (
        "width: 70; textFormat: Text.StyledText; elide: Text.ElideMiddle",
        false,
    ),
    (
        "width: 70; textFormat: Text.RichText; elide: Text.ElideRight",
        false,
    ),
];

/// (a family whose font has a full stop and no U+2026, a line in its own script)
const NO_ELLIPSIS: [(&str, &str); 4] = [
    (
        "Noto Sans Coptic",
        "\u{2c80}\u{2c82}\u{2c84} \u{2c86}\u{2c88}\u{2c8a} \u{2c8c}\u{2c8e}\u{2c90} \u{2c92}\u{2c94}\u{2c96} \u{2c98}\u{2c9a}",
    ),
    (
        "Noto Sans Cham",
        "\u{aa00}\u{aa01}\u{aa02} \u{aa06}\u{aa07}\u{aa08} \u{aa09}\u{aa0a}\u{aa0b} \u{aa0c}\u{aa0d}\u{aa0e}",
    ),
    (
        "Noto Naskh Arabic",
        "\u{0645}\u{0631}\u{062d}\u{0628}\u{0627} \u{0628}\u{0627}\u{0644}\u{0639}\u{0627}\u{0644}\u{0645} \u{0633}\u{0644}\u{0627}\u{0645} \u{0643}\u{062a}\u{0627}\u{0628}",
    ),
    (
        "Noto Sans Vithkuqi",
        "\u{10570}\u{10571}\u{10572} \u{10573}\u{10574}\u{10575} \u{10576}\u{10577}\u{10578} \u{10579}\u{1057a}",
    ),
];

/// A font with no U+2026 and a full stop: Qt cuts a line off with three full stops of that
/// font (QTextEngine::elidedText), so that the ellipsis of another font does not move the
/// line. Ours put another font's U+2026 there.
#[test]
fn font_without_ellipsis_is_cut_off_with_three_dots_as_qt() {
    if !testenv::require("qml6") {
        return;
    }
    let mut wrong = vec![];
    let mut cases = 0;
    for (family, own) in NO_ELLIPSIS {
        for text in [WRAPPED, own] {
            wrong.extend(lines_differing(family, text, &ELIDED));
            cases += STYLES.len() * ELIDED.len();
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {cases} elided cases differ from Qt (lineCount, contentWidth, contentHeight, \
         implicitWidth, implicitHeight, truncated):\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

/// `compare` in a process whose fontconfig has the fixture `file` and no other user font.
fn compare_bitmap(test: &str, file: &str, family: &str) {
    if !testenv::require("qml6") || !qt_gl::available() {
        return;
    }
    if !testenv::in_own_process(test) {
        let dir = tempfile::tempdir().unwrap();
        let (data, cache) = (dir.path().join("data"), dir.path().join("cache"));
        std::fs::create_dir_all(data.join("fonts")).unwrap();
        let fixture = format!(
            "{}/../qsg/tests/fixtures/bitmap/{file}",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::copy(fixture, data.join("fonts").join(file)).unwrap();
        testenv::run_in_own_process(test, [("XDG_DATA_HOME", data), ("XDG_CACHE_HOME", cache)]);
        return;
    }
    // The strike's own: the advances of A, B, W, i, j and m, its ascent 12 and descent 4.
    compare(family, Some((69.0, 16.0, 12.0)));
}

#[test]
fn bitmap_font_bdf_matches_qt() {
    const TEST: &str = concat!(module_path!(), "::bitmap_font_bdf_matches_qt");
    compare_bitmap(TEST, "impulse-bitmap-test.bdf", "Impulse Bitmap Test");
}

/// FreeType names a PCF file's family with its foundry first.
#[test]
fn bitmap_font_pcf_matches_qt() {
    const TEST: &str = concat!(module_path!(), "::bitmap_font_pcf_matches_qt");
    compare_bitmap(
        TEST,
        "impulse-bitmap-test.pcf",
        "impulse Impulse Bitmap Test",
    );
}

#[test]
fn bitmap_font_otb_matches_qt() {
    const TEST: &str = concat!(module_path!(), "::bitmap_font_otb_matches_qt");
    compare_bitmap(TEST, "impulse-bitmap-test.otb", "Impulse Bitmap Test");
}

fn compare_outline(family: &str) {
    if testenv::require("qml6") && qt_gl::available() {
        compare(family, None);
    }
}

#[test]
fn noto_sans_matches_qt() {
    compare_outline("Noto Sans");
}

#[test]
fn noto_serif_matches_qt() {
    compare_outline("Noto Serif");
}

#[test]
fn dejavu_sans_mono_matches_qt() {
    compare_outline("DejaVu Sans Mono");
}
