//! A Qt5Compat `DropShadow` whose `color` is not a whole number of 255ths, against Qt itself
//! through OpenGL (support/qt_window.rs), with no tolerance. The colour is a uniform of the blur's
//! shader: Qt hands it over as the QColor's floats (16 bits a channel), so `Qt.rgba(0, 0, 0, 0.3)`
//! (the shell's `Appearance.colors.colShadow`) is 0.30001 there and not 77/255.

#[path = "support/qt_window.rs"]
mod qt_window;

const WHITE: &str = "Rectangle { anchors.fill: parent; color: \"#ffffff\" }\n";
/// Every column its own colour: what the shadow darkens differs from pixel to pixel.
const STRIPES: &str = "Repeater { model: 160; Rectangle { required property int index; x: index; \
     width: 1; height: 160; \
     color: Qt.rgba(index / 159, ((index * 7) % 160) / 159, ((index * 13) % 160) / 159, 1) } }\n";

fn scene(under: &str, colour: &str) -> String {
    format!(
        "{under}Rectangle {{ id: src; x: 30; y: 30; width: 100; height: 100; radius: 20; \
             color: \"#73342d\"; visible: false }}\n\
         DropShadow {{ source: src; anchors.fill: src; radius: 8; samples: 17; \
             color: {colour}; transparentBorder: true }}\n"
    )
}

fn like_qt(under: &str, colour: &str) {
    qt_window::like_qt(&scene(under, colour), (160, 160), &[1.0], 0, 0);
}

#[test]
fn a_shadow_of_eight_bit_colour_has_qt_s_pixels() {
    like_qt(WHITE, "\"#4d000000\"");
}

#[test]
fn a_shadow_of_three_tenths_alpha_has_qt_s_pixels_on_white() {
    like_qt(WHITE, "Qt.rgba(0, 0, 0, 0.3)");
}

#[test]
fn a_shadow_of_three_tenths_alpha_has_qt_s_pixels_on_stripes() {
    like_qt(STRIPES, "Qt.rgba(0, 0, 0, 0.3)");
}

#[test]
fn a_shadow_of_fractional_channels_has_qt_s_pixels() {
    like_qt(WHITE, "Qt.rgba(0.21, 0.502, 0.698, 0.45)");
}
