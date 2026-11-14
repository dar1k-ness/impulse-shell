//! The caret of a TextArea, against Qt 6.12 itself (support/qt_window.rs, Basic style).
//!
//! `cursorRectangle` is a document line high, leading included, with or without text. An empty
//! TextArea is `QFontMetricsF::height()` high less its paddings (QQuickTextEdit::updateSize),
//! which has no leading, and QQuickTextArea::updatePaintNode clips what the editor draws to the
//! field less its paddings: with a font that has leading (the original's Google Sans Flex at 15 px: 18
//! and 19; here DejaVu Math TeX Gyre: 15 and 18) Qt draws the caret of the empty field shorter
//! than `cursorRectangle`, and the caret of a field with text whole. The input of the original's
//! AssistantChat.qml drew it one row longer than under Quickshell.
//!
//! Qt's caret blinks while `cursorVisible`, with or without focus; the shots are taken with
//! one that does not (`qt_window::both_with_a_still_caret`).
//!
//! And its tabs: no glyph, a gap to the next tab stop, in the text and in a selection.
//!
//! With `TEXT_AREA_CARET_SHOTS=DIR` every scene is also written to DIR as `NAME-qt.png` and
//! `NAME-ours.png`.

#[path = "support/qt_window.rs"]
mod qt_window;

use qsg::raster::Canvas;

const BACK: [u8; 3] = [0x20, 0x20, 0x20];
/// The hint is too dim to count as ink.
const DIM: &str = "#303030";
const FIELD: &str = "wrapMode: TextEdit.Wrap; padding: 10; background: null; cursorVisible: true\n\
    font.family: \"DejaVu Math TeX Gyre\"; font.pixelSize: 15; color: \"white\"";

/// The tallest column of pixels that are clearly not the background, which is the caret:
/// (x, first row, rows).
fn caret(c: &Canvas) -> (usize, usize, usize) {
    let ink = |x: usize, y: usize| (0..3).any(|n| c.px[y * c.w + x][n].abs_diff(BACK[n]) > 64);
    (0..c.w)
        .map(|x| {
            let rows: Vec<usize> = (0..c.h).filter(|&y| ink(x, y)).collect();
            (x, rows.first().copied().unwrap_or(0), rows.len())
        })
        .max_by_key(|&(x, _, rows)| (rows, std::cmp::Reverse(x)))
        .unwrap()
}

fn same_caret(name: &str, body: &str) {
    let Some((qt, ours)) = qt_window::both_with_a_still_caret(body, (240, 140), 1.0) else {
        return;
    };
    if let Some(dir) = std::env::var_os("TEXT_AREA_CARET_SHOTS") {
        let dir = std::path::Path::new(&dir);
        qt.save_png(dir.join(format!("{name}-qt.png")).to_str().unwrap())
            .unwrap();
        ours.save_png(dir.join(format!("{name}-ours.png")).to_str().unwrap())
            .unwrap();
    }
    assert!(
        caret(&qt).2 > 8,
        "{name}: Qt draws no caret, the scene is wrong"
    );
    assert_eq!(
        caret(&ours),
        caret(&qt),
        "{name}: column, first row and rows of the caret, ours and Qt's"
    );
}

/// The input of AssistantChat.qml: the view is as high as its field, the hint is a Text of
/// its own.
fn chat(text: &str) -> String {
    format!(
        "ScrollView {{ id: v; x: 10; y: 10; width: 200; height: Math.min(100, t.height); clip: true\n\
         TextArea {{ id: t; anchors.fill: parent; {FIELD}; text: \"{text}\" }} }}\n\
         Text {{ x: 40; y: 20; visible: t.text.length === 0; text: \"Message the agent\"\n\
         font: t.font; color: \"{DIM}\" }}"
    )
}

#[test]
fn the_caret_of_an_empty_field_with_a_hint_in_a_scroll_view() {
    same_caret("chat-empty", &chat(""));
}

#[test]
fn the_caret_of_a_field_with_text_in_a_scroll_view() {
    same_caret("chat-text", &chat("    ab"));
}

/// The same with the hint as the field's own `placeholderText`.
#[test]
fn the_caret_of_an_empty_field_with_a_placeholder_in_a_scroll_view() {
    let body = format!(
        "ScrollView {{ x: 10; y: 10; width: 200\n\
         TextArea {{ {FIELD}; placeholderText: \"Message the agent\"\n\
         placeholderTextColor: \"{DIM}\" }} }}"
    );
    same_caret("view-placeholder", &body);
}

/// The same clip of a TextArea that is in no Flickable.
#[test]
fn the_caret_of_an_empty_field_with_a_placeholder() {
    let body = format!(
        "TextArea {{ x: 10; y: 10; width: 200; {FIELD}; placeholderText: \"Message the agent\"\n\
         placeholderTextColor: \"{DIM}\" }}"
    );
    same_caret("alone-placeholder", &body);
}

#[test]
fn the_caret_of_a_field_with_text() {
    let body = format!("TextArea {{ x: 10; y: 10; width: 200; {FIELD}; text: \"    ab\" }}");
    same_caret("alone-text", &body);
}

/// Whether each pixel is `kind`, and how many pixels of `a` that are have none that is within
/// a pixel of them in `b`, and the other way round: what one side draws and the other does not.
fn apart(a: &Canvas, b: &Canvas, kind: impl Fn(&[u8; 4]) -> bool) -> usize {
    let mask = |c: &Canvas| c.px.iter().map(&kind).collect::<Vec<bool>>();
    let (ma, mb) = (mask(a), mask(b));
    let lone = |m: &[bool], other: &[bool]| {
        (0..a.h)
            .flat_map(|y| (0..a.w).map(move |x| (x, y)))
            .filter(|&(x, y)| m[y * a.w + x])
            .filter(|&(x, y)| {
                let near = |x: usize, y: usize| x < a.w && y < a.h && other[y * a.w + x];
                !(y.saturating_sub(1)..=y + 1)
                    .any(|v| (x.saturating_sub(1)..=x + 1).any(|u| near(u, v)))
            })
            .count()
    };
    assert_eq!((a.w, a.h), (b.w, b.h), "device size");
    assert!(ma.iter().any(|&m| m), "Qt draws none, the scene is wrong");
    lone(&ma, &mb) + lone(&mb, &ma)
}

/// A tab in an editor is no glyph: what follows it starts at the next tab stop, 80 px apart
/// (QTextEngine::calculateTabWidth). Ours drew the font's box for it and no gap, so the input
/// of AssistantChat.qml and an answer indented with tabs read `a▯b`.
#[test]
fn a_tab_is_a_gap_to_the_next_tab_stop() {
    let body = format!(
        "TextArea {{ x: 10; y: 10; width: 220; {FIELD}\n\
         text: \"a\\tb\\tc\\nlong word\\tx\\n\\tif x:\\n\\t\\treturn 1\" }}"
    );
    let Some((qt, ours)) = qt_window::both_with_a_still_caret(&body, (240, 140), 1.0) else {
        return;
    };
    qt_window::dump(&ours, &qt, 1.0);
    let ink = |p: &[u8; 4]| (0..3).any(|n| p[n].abs_diff(BACK[n]) > 64);
    assert_eq!(
        apart(&qt, &ours, ink),
        0,
        "ink of one side and not the other"
    );
}

/// A selection across tabs is one band from its first letter to its last: the tabs between
/// letters are in it, the tab it ends with is not (QQuickTextNodeEngine::processCurrentLine
/// unites the selected glyph runs, and a tab has none).
#[test]
fn a_selection_across_tabs() {
    let body = format!(
        "TextArea {{ x: 10; y: 10; width: 220; {FIELD}; text: \"a\\tb\\tc\\tdef\"\n\
         selectionColor: \"#00aa00\"; selectedTextColor: \"white\"\n\
         Component.onCompleted: select(0, 4) }}"
    );
    let Some((qt, ours)) = qt_window::both_with_a_still_caret(&body, (240, 140), 1.0) else {
        return;
    };
    qt_window::dump(&ours, &qt, 1.0);
    let band = |p: &[u8; 4]| p[1] > 0x90 && p[0] < 0x40 && p[2] < 0x40;
    assert_eq!(
        apart(&qt, &ours, band),
        0,
        "selection of one side and not the other"
    );
}
