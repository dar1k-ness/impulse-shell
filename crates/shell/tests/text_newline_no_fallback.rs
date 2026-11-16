//! A line feed is never looked up in the fallback fonts (QFontEngineMulti::stringToCMap leaves
//! QFontEngine::isIgnorableChar characters to the first engine). A two-line Text whose text
//! has a character from U+0590 on (the em dash of "~ : fish — Konsole") is shaped as parts
//! of the whole text, line feed included; looking that up opened every fallback font, since
//! none has it: 240 font files in the frame the overview first showed such a window title.

use qml::runtime::Runtime;
use shell::scene;

/// The fallback fonts opened by laying out a Text with `text`.
fn fallbacks_opened(text: &str) -> usize {
    let mut rt = Runtime::new(None);
    let mut fonts = scene::Fonts::default();
    let root = rt
        .load_str(&format!(
            "import QtQuick\nItem {{ width: 400; height: 300; Text {{ text: {text:?} }} }}"
        ))
        .unwrap();
    rt.settle();
    scene::layout_window(&mut rt, &mut fonts, root, [400.0, 300.0]);
    let stats = fonts.stats();
    let n = stats.rsplit("fallbacks=").next().unwrap();
    n.parse().unwrap_or_else(|_| panic!("no count in {stats}"))
}

#[test]
fn a_line_feed_opens_no_fallback_font() {
    let one_line = fallbacks_opened("~ : fish — Konsole [org.kde.konsole]");
    for sep in ["\n", "\r", "\u{2028}", "\u{2029}", "\u{1}"] {
        let text = format!("~ : fish — Konsole{sep}[org.kde.konsole]");
        assert_eq!(fallbacks_opened(&text), one_line, "{text:?}");
    }
}
