//! Where a Text breaks its lines, what it cuts off and what it reports, compared with Qt
//! itself (`qml6`, offscreen) on the fonts of the machine the test runs on: line breaking at
//! the Unicode line break opportunities (UAX #14) and anywhere, tabs and carriage returns,
//! white space at a break, elision by width, by height and by `maximumLineCount`, and
//! `truncated`. Per case: `lineCount`, `contentWidth`, `contentHeight`, `implicitWidth`,
//! `implicitHeight`, `truncated`, and for text that is not cut off the width of every line.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

/// Texts with a break opportunity of every kind the shell's strings have, as QML string
/// literals: Latin, Cyrillic, `/`, `-`, `.`, `,`, `_`, no-break space (as before); Chinese,
/// Japanese, U+200B, tabs, dashes, `|`, a URL's `?` and `&`, soft hyphens; white space at a
/// break and before a line feed; CRLF.
const TEXTS: [&str; 22] = [
    "alpha beta gamma delta epsilon",
    "съешь ещё этих мягких булок",
    "usr/share/fonts/truetype/dejavu",
    "well-known-dash-separated-words",
    "org.kde.plasma.desktop.config",
    "one,two,three,four,five,six",
    "snake_case_identifier_here",
    "no\\u00A0break\\u00A0space here and there",
    "中文文本需要换行中文文本需要换行",
    "これは日本語のテキストです。折り返しが必要です",
    "zero\\u200Bwidth\\u200Bspace\\u200Bbreaks\\u200Bhere",
    "tab\\tseparated\\twords\\there",
    "em—dash—separated—words",
    "en–dash–separated–words",
    "pipe|separated|words|here",
    "https://example.com/path?with=query&and=more",
    "soft\\u00ADhyphen\\u00ADated\\u00ADword",
    "Supercalifragilisticexpialidocious end",
    "alpha beta \\ngamma",
    "alpha  beta   gamma    delta",
    "one\\r\\ntwo three\\r\\nfour",
    "  leading and trailing  ",
];

const STYLED: [&str; 6] = [
    "wrap <i>italic words wrap</i> and <i>end italif</i>",
    "<b>Supercalifragilisticexpialidocious</b> end",
    "alpha <b>beta</b> gamma delta epsilon zeta",
    "plain <b>中文文本需要</b>换行 <u>under</u>",
    "one<br>two <u>three four</u> five",
    "a <font color=\\\"red\\\">red/slash-dash</font> tail",
];

/// (what it checks, the Text's properties, whether every line's width is compared too)
type Case = (String, String, bool);

fn cases() -> Vec<Case> {
    let mut cases: Vec<Case> = Vec::new();
    let mut add = |what: &str, props: String, lines: bool| {
        cases.push((what.to_string(), props, lines));
    };
    // Findings 5, 7, 8: where lines break, in each wrap mode at three widths.
    for text in TEXTS {
        for mode in ["WordWrap", "Wrap", "WrapAnywhere"] {
            for width in [60, 100, 150] {
                add(
                    if mode == "WrapAnywhere" {
                        "wrap anywhere"
                    } else {
                        "wrap"
                    },
                    format!("width: {width}; wrapMode: Text.{mode}; text: \"{text}\""),
                    true,
                );
            }
        }
        // Finding 6: nothing to wrap, so only what is drawn and what is not counts.
        add("no wrap", format!("text: \"{text}\""), true);
    }
    // An italic line is as wide as its last glyph leans: the family's italic face, as in Qt.
    for width in [60, 100, 150] {
        add(
            "wrap",
            format!(
                "width: {width}; wrapMode: Text.Wrap; font.italic: true; \
                 text: \"italic words wrap italif\""
            ),
            true,
        );
    }
    // The space `font.letterSpacing` puts after every character counts toward the line's width.
    for width in [60, 100, 150] {
        for rest in ["wrapMode: Text.Wrap", "elide: Text.ElideRight"] {
            add(
                "wrap",
                format!(
                    "width: {width}; {rest}; font.letterSpacing: 3; \
                     text: \"spaced words wrap office\""
                ),
                rest.starts_with("wrap"),
            );
        }
    }
    // Finding 6: tab stops and carriage returns.
    for text in [
        "a\\tb",
        "a\\tb\\tc",
        "\\tindent",
        "wide wide wide\\ttab",
        "one\\r\\ntwo",
        "one\\rtwo",
        "trailing\\n",
        "two\\n\\n",
        "\\n",
    ] {
        add("tab and CR", format!("text: \"{text}\""), true);
    }
    // Finding 7: white space at a break does not shift an aligned line.
    for align in ["AlignHCenter", "AlignRight"] {
        add(
            "space at a break",
            format!(
                "width: 100; wrapMode: Text.Wrap; horizontalAlignment: Text.{align}; \
                 text: \"alpha  beta gamma  delta\""
            ),
            true,
        );
    }
    // Finding 2: styled and rich text break a long word like plain text does.
    for text in STYLED {
        for format in ["StyledText", "RichText"] {
            for mode in ["WordWrap", "Wrap", "WrapAnywhere"] {
                for width in [60, 100] {
                    add(
                        "styled wrap",
                        format!(
                            "width: {width}; wrapMode: Text.{mode}; textFormat: Text.{format}; \
                             text: \"{text}\""
                        ),
                        // A QTextDocument reports no lines.
                        format == "StyledText",
                    );
                }
            }
        }
    }
    // Finding 3: the ellipsis of styled text stays inside the width.
    for text in STYLED {
        for width in [60, 80, 100, 111] {
            for lines in [1, 2] {
                add(
                    "styled elide",
                    format!(
                        "width: {width}; wrapMode: Text.Wrap; maximumLineCount: {lines}; \
                         elide: Text.ElideRight; textFormat: Text.StyledText; text: \"{text}\""
                    ),
                    false,
                );
            }
            add(
                "styled elide",
                format!(
                    "width: {width}; elide: Text.ElideRight; textFormat: Text.StyledText; \
                     text: \"{text}\""
                ),
                false,
            );
        }
    }
    // Styled text is elided on the left and in the middle too; a rich text document never.
    for text in STYLED {
        for elide in ["ElideLeft", "ElideMiddle"] {
            for width in [60, 80, 100, 111] {
                for format in ["StyledText", "RichText"] {
                    add(
                        "styled elide sides",
                        format!(
                            "width: {width}; elide: Text.{elide}; textFormat: Text.{format}; \
                             text: \"{text}\""
                        ),
                        false,
                    );
                }
            }
        }
    }
    // Finding 4: no room for the ellipsis draws nothing.
    for elide in ["ElideRight", "ElideLeft", "ElideMiddle"] {
        for width in [0, 5, 14, 30] {
            add(
                "elide without room",
                format!("width: {width}; elide: Text.{elide}; text: \"alpha beta gamma\""),
                false,
            );
        }
    }
    add(
        "elide without room",
        "width: 0; elide: Text.ElideRight; textFormat: Text.StyledText; text: \"a <b>b</b> c\""
            .to_string(),
        false,
    );
    // Finding 9: wrapped and elided text is cut by height.
    let long = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu";
    for height in [0, 10, 30, 44, 50, 70, 300] {
        for mode in ["Wrap", "WordWrap", "WrapAnywhere"] {
            add(
                "cut by height",
                format!(
                    "width: 100; height: {height}; wrapMode: Text.{mode}; \
                     elide: Text.ElideRight; text: \"{long}\""
                ),
                false,
            );
        }
        add(
            "cut by height",
            format!(
                "width: 100; height: {height}; wrapMode: Text.Wrap; elide: Text.ElideRight; \
                 textFormat: Text.StyledText; text: \"alpha <b>beta gamma</b> delta epsilon zeta eta\""
            ),
            false,
        );
        add(
            "cut by height",
            format!(
                "width: 100; height: {height}; wrapMode: Text.Wrap; elide: Text.ElideRight; \
                 textFormat: Text.RichText; text: \"alpha <b>beta gamma</b> delta epsilon zeta eta\""
            ),
            false,
        );
    }
    add(
        "cut by height",
        format!("width: 100; height: 44; wrapMode: Text.Wrap; text: \"{long}\""),
        false,
    );
    add(
        "cut by height",
        "width: 100; height: 44; elide: Text.ElideRight; text: \"one\\ntwo\\nthree\\nfour\""
            .to_string(),
        false,
    );
    // Finding 10: maximumLineCount drops the other lines; with elide the last one is elided.
    for lines in [1, 2, 3, 9] {
        for elide in ["", "elide: Text.ElideRight; "] {
            for mode in ["Wrap", "WordWrap", "WrapAnywhere"] {
                add(
                    "maximumLineCount",
                    format!(
                        "width: 100; maximumLineCount: {lines}; {elide}wrapMode: Text.{mode}; \
                         text: \"{long}\""
                    ),
                    false,
                );
            }
            add(
                "maximumLineCount",
                format!(
                    "width: 100; maximumLineCount: {lines}; {elide}\
                     text: \"one\\ntwo is a longer line\\nthree\\nfour\""
                ),
                false,
            );
            add(
                "maximumLineCount",
                format!(
                    "maximumLineCount: {lines}; {elide}text: \"one\\ntwo is a longer line\\nthree\""
                ),
                false,
            );
        }
    }
    // Finding 11: elision by width is for a single line, and `truncated` tells of it.
    for elide in ["ElideRight", "ElideLeft", "ElideMiddle"] {
        for text in [
            "a single line that is too long",
            "fits",
            "first line is too long\\nsecond",
            "中文文本需要换行中文文本需要换行",
        ] {
            for width in [60, 100] {
                add(
                    "elide by width",
                    format!("width: {width}; elide: Text.{elide}; text: \"{text}\""),
                    false,
                );
            }
        }
    }
    // Elision is by character, also inside a ligature ("fi" in DejaVu Sans).
    for elide in ["ElideRight", "ElideLeft", "ElideMiddle"] {
        for text in ["first words and the last", "fififififififififi"] {
            for width in [40, 55, 70, 85] {
                add(
                    "elide by width",
                    format!(
                        "font.family: \"DejaVu Sans\"; font.pixelSize: 14; width: {width}; \
                         elide: Text.{elide}; text: \"{text}\""
                    ),
                    false,
                );
            }
        }
    }
    cases
}

/// What is compared of every case.
const PROPS: [&str; 6] = [
    "lineCount",
    "contentWidth",
    "contentHeight",
    "implicitWidth",
    "implicitHeight",
    "truncated",
];

const FONT: &str = "font.family: \"Noto Sans\"; font.pixelSize: 16";

/// Per case a Text, and for Qt a twin whose `lineLaidOut` reports every line's width.
/// What is compared is bound from the start, in Qt as here: an eliding Text without room is
/// laid out differently once its implicit size is read (`qt_geometry/elide_no_room.qml` has
/// both orders).
fn source(qt: bool) -> String {
    let mut s = String::from("import QtQuick\nItem {\n    id: root\n    property var rows: []\n");
    s += "    function laid(i, line) {\n        if (line.number === 0)\n            \
          rows[i] = [];\n        rows[i].push(line.implicitWidth);\n    }\n";
    let cases = cases();
    for (i, (_, props, lines)) in cases.iter().enumerate() {
        let font = if props.contains("font.family") {
            String::new()
        } else {
            format!("{FONT}; ")
        };
        s += &format!("    Text {{ id: t{i}; {font}{props} }}\n");
        if qt && *lines {
            s += &format!(
                "    Text {{ {font}{props}; onLineLaidOut: line => root.laid({i}, line) }}\n"
            );
        }
        if !qt {
            s += &format!("    property Item item{i}: t{i}\n");
        }
        for (k, prop) in PROPS.iter().enumerate() {
            s += &format!("    property real v{i}_{k}: t{i}.{prop}\n");
        }
    }
    if qt {
        s += &format!(
            "    Component.onCompleted: {{\n        for (let i = 0; i < {}; i++)\n            \
             console.log(\"CASE\", i, [0, 1, 2, 3, 4, 5].map(k => root[\"v\" + i + \"_\" + k])\
             .join(\" \") + \" |\" + (rows[i] ? \" \" + rows[i].join(\" \") : \"\"));\n        \
             Qt.quit();\n    }}\n",
            cases.len()
        );
    }
    s += "}\n";
    s
}

/// ([lineCount, contentWidth, contentHeight, implicitWidth, implicitHeight, truncated], the
/// width of every line)
type Laid = (Vec<f64>, Vec<f64>);

fn numbers(s: &str) -> Vec<f64> {
    s.split_whitespace().map(|n| n.parse().unwrap()).collect()
}

fn qt_laid() -> Vec<Laid> {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("wrap.qml");
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
        .map(|(props, lines)| (numbers(props), numbers(lines)))
        .collect();
    assert_eq!(laid.len(), cases().len(), "qml6 printed:\n{log}");
    laid
}

fn our_laid() -> Vec<Laid> {
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
    cases()
        .iter()
        .enumerate()
        .map(|(i, (_, _, lines))| {
            let props = (0..PROPS.len())
                .map(|k| {
                    w.read(root, &format!("v{i}_{k}"))
                        .map_or(f64::NAN, |v| v.to_number())
                })
                .collect();
            let widths = match w.read(root, &format!("item{i}")) {
                Some(Value::Item(t)) if *lines => {
                    scene::text_line_widths(&mut w, &mut fonts.borrow_mut(), t)
                }
                _ => vec![],
            };
            (props, widths)
        })
        .collect()
}

/// The cases of the groups `what` (all of them when empty) measure in ours what they do in Qt.
fn check(what: &[&str]) {
    if !testenv::require("qml6") {
        return;
    }
    let (qt, ours) = (qt_laid(), our_laid());
    let cases = cases();
    let checked: Vec<_> = cases
        .iter()
        .zip(qt.iter().zip(&ours))
        .filter(|(case, _)| what.is_empty() || what.contains(&case.0.as_str()))
        .collect();
    assert!(!checked.is_empty());
    let wrong: Vec<String> = checked
        .iter()
        .filter(|(_, (qt, ours))| qt != ours)
        .map(|((what, props, _), (qt, ours))| {
            format!("{what}: Text {{ {props} }}\n    ours {ours:?}\n    Qt   {qt:?}")
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "{} of {} cases differ from Qt (lineCount, contentWidth, contentHeight, implicitWidth, \
         implicitHeight, truncated; line widths):\n{}",
        wrong.len(),
        checked.len(),
        wrong.join("\n")
    );
}

#[test]
fn text_lines_match_qt() {
    check(&[]);
}

/// Finding 2: styled and rich text with `Text.Wrap` break a word wider than the item.
#[test]
fn styled_text_breaks_a_long_word() {
    check(&["styled wrap"]);
}

/// Finding 3: the ellipsis of styled text stays inside the item.
#[test]
fn styled_text_is_elided_inside_its_width() {
    check(&["styled elide"]);
}

/// Styled text is elided on the left and in the middle as Qt elides it; a rich text document
/// is not elided.
#[test]
fn styled_text_is_elided_on_either_side() {
    check(&["styled elide sides"]);
}

/// Finding 4: elided text with no room for the ellipsis draws nothing.
#[test]
fn elided_text_without_room_is_empty() {
    check(&["elide without room"]);
}

/// Finding 5: lines break at the Unicode line break opportunities.
#[test]
fn lines_break_at_unicode_opportunities() {
    check(&["wrap"]);
}

/// Finding 6: a tab moves to the next tab stop and a carriage return draws nothing.
#[test]
fn tabs_and_carriage_returns_are_not_glyphs() {
    check(&["tab and CR", "no wrap"]);
}

/// Finding 7: white space a line breaks at is not part of the line.
#[test]
fn space_at_a_break_is_not_laid_out() {
    check(&["space at a break"]);
}

/// Finding 8: `Text.WrapAnywhere` breaks between any two characters.
#[test]
fn wrap_anywhere_breaks_inside_words() {
    check(&["wrap anywhere"]);
}

/// Finding 9: wrapped and elided text shows the lines that fit its height.
#[test]
fn elided_text_is_cut_by_height() {
    check(&["cut by height"]);
}

/// Finding 10: `maximumLineCount` drops the lines past it.
#[test]
fn maximum_line_count_drops_the_rest() {
    check(&["maximumLineCount"]);
}

/// Finding 11: `truncated` tells of text elided by width.
#[test]
fn truncated_tells_of_elided_text() {
    check(&["elide by width"]);
}
