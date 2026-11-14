//! Coloured `Text.NativeRendering` text against Qt itself through OpenGL (support/qt_window.rs),
//! with no tolerance: glyph coverage times the text colour, blended over the background.

#[path = "support/qt_window.rs"]
mod qt_window;

/// Colours whose channels times a glyph's coverage land just above a half of 1/255 for many
/// coverages (216 * 245 / 255 = 207.53): a coverage that reaches the multiplication up to
/// 1/2048 of itself short (a half float, cut off) gives a channel 1 of 255 lower than Qt's.
#[test]
fn coloured_native_text_has_qt_s_pixels() {
    qt_window::like_qt(
        "Text { x: 6; y: 4; renderType: Text.NativeRendering; font.family: \"DejaVu Sans\"; \
             font.pixelSize: 15; color: \"#d8c2bf\"; text: \"No media Workspace 12:34\" }\n\
         Text { x: 6; y: 28; renderType: Text.NativeRendering; font.family: \"DejaVu Sans\"; \
             font.pixelSize: 28; color: \"#c07a72\"; text: \"Sphinx of quartz\" }\n\
         Text { x: 6; y: 66; renderType: Text.NativeRendering; font.family: \"DejaVu Sans\"; \
             font.pixelSize: 12; color: \"#8a6f6c\"; opacity: 0.7; text: \"judge my vow 0123456789\" }",
        (260, 92),
        &[1.0],
        0,
        0,
    );
}
