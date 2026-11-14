//! `Text.NativeRendering` text larger than Qt's glyph cache limit against Qt itself through
//! OpenGL (support/qt_window.rs), with no tolerance. QFontEngineFT hints glyphs up to
//! QFontEngine::maxCachedGlyphSize() (64) pixels and loads larger ones unhinted.

#[path = "support/qt_window.rs"]
mod qt_window;

/// White text: 255 times a glyph's coverage is the coverage, so only the glyphs' shapes and
/// places decide.
fn like_qt(size: u32, family: &str) {
    let body = format!(
        "Text {{ x: 6; y: 4; renderType: Text.NativeRendering; font.family: \"{family}\"; \
         font.pixelSize: {size}; color: \"white\"; text: \"1234\" }}\n\
         Text {{ x: 6; y: 112; renderType: Text.NativeRendering; font.family: \"{family}\"; \
         font.pixelSize: {size}; color: \"white\"; text: \"Sphinx, vow\" }}"
    );
    qt_window::like_qt(&body, (640, 232), &[1.0], 0, 0);
}

#[test]
fn up_to_64_pixels_is_hinted() {
    like_qt(64, "DejaVu Sans");
}

#[test]
fn above_64_pixels_is_not_hinted() {
    like_qt(65, "DejaVu Sans");
    like_qt(68, "Noto Sans");
    like_qt(81, "DejaVu Sans");
}
