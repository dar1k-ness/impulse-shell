//! Effects whose `color` is not a whole number of 255ths, against Qt itself through OpenGL
//! (support/qt_window.rs), with no tolerance, on our GPU path and on our CPU path. The colour is
//! a uniform of the effect's shader: Qt hands it over as the QColor's floats (16 bits a
//! channel), so `Qt.rgba(0, 0, 0, 0.3)` (the shell's `Appearance.colors.colShadow`, the colour
//! of every `StyledRectangularShadow`) is 0.30001 there and not 77/255, and the tenth of alpha
//! of the dock's and the tray's `ColorOverlay` is 0.1 and not 26/255.

#[path = "support/qt_window.rs"]
mod qt_window;

const SIZE: (usize, usize) = (160, 160);
const WHITE: &str = "Rectangle { anchors.fill: parent; color: \"#ffffff\" }\n";
/// Every column its own colour: what the effect covers differs from pixel to pixel.
const STRIPES: &str = "Repeater { model: 160; Rectangle { required property int index; x: index; \
     width: 1; height: 160; \
     color: Qt.rgba(index / 159, ((index * 7) % 160) / 159, ((index * 13) % 160) / 159, 1) } }\n";

/// A `ColorOverlay` over an item of many colours, as over the dock's and the tray's icons.
fn overlay(colour: &str) -> String {
    format!(
        "Item {{ id: src; x: 16; y: 16; width: 128; height: 128; visible: false\n\
             Repeater {{ model: 64; Rectangle {{ required property int index; \
                 x: (index % 8) * 16; y: Math.floor(index / 8) * 16; width: 16; height: 16; \
                 color: Qt.rgba((index % 8) / 7, Math.floor(index / 8) / 7, \
                     ((index * 5) % 64) / 63, 1) }} }} }}\n\
         ColorOverlay {{ anchors.fill: src; source: src; color: {colour} }}\n"
    )
}

/// Colorizer.qml's MultiEffect over the same item.
fn colorized(colour: &str) -> String {
    overlay("\"#000000\"").replace(
        "ColorOverlay { anchors.fill: src; source: src; color: \"#000000\" }",
        &format!(
            "MultiEffect {{ anchors.fill: src; source: src; colorization: 1; brightness: 0.2; \
                 colorizationColor: {colour} }}"
        ),
    )
}

/// StyledRectangularShadow.qml's shadow (`cached` as there, or not) under a card.
fn shadow(under: &str, colour: &str, cached: bool) -> String {
    format!(
        "{under}RectangularShadow {{ anchors.fill: card; radius: card.radius; blur: 9; \
             offset: Qt.vector2d(0.0, 1.0); spread: 1; color: {colour}; cached: {cached} }}\n\
         Rectangle {{ id: card; x: 30; y: 30; width: 100; height: 100; radius: 20; \
             color: \"#73342d\" }}\n"
    )
}

fn gpu(body: &str) {
    qt_window::like_qt(body, SIZE, &[1.0], 0, 0);
}

fn cpu(body: &str) {
    qt_window::cpu_like_qt(body, SIZE);
}

#[test]
fn an_overlay_of_eight_bit_colour_has_qt_s_pixels_on_the_gpu() {
    gpu(&overlay("\"#1ae6e0e9\""));
}

#[test]
fn an_overlay_of_eight_bit_colour_has_qt_s_pixels_on_the_cpu() {
    cpu(&overlay("\"#1ae6e0e9\""));
}

#[test]
fn an_overlay_of_a_tenth_of_alpha_has_qt_s_pixels_on_the_gpu() {
    gpu(&overlay("Qt.rgba(0.902, 0.878, 0.914, 0.1)"));
}

#[test]
fn an_overlay_of_a_tenth_of_alpha_has_qt_s_pixels_on_the_cpu() {
    cpu(&overlay("Qt.rgba(0.902, 0.878, 0.914, 0.1)"));
}

#[test]
fn a_colorization_of_eight_bit_colour_has_qt_s_pixels_on_the_gpu() {
    gpu(&colorized("\"#e6e0e9\""));
}

#[test]
fn a_colorization_of_eight_bit_colour_has_qt_s_pixels_on_the_cpu() {
    cpu(&colorized("\"#e6e0e9\""));
}

#[test]
fn a_colorization_of_fractional_colour_has_qt_s_pixels_on_the_gpu() {
    gpu(&colorized("Qt.rgba(0.9, 0.876, 0.912, 0.7)"));
}

#[test]
fn a_colorization_of_fractional_colour_has_qt_s_pixels_on_the_cpu() {
    cpu(&colorized("Qt.rgba(0.9, 0.876, 0.912, 0.7)"));
}

#[test]
fn a_rectangular_shadow_of_eight_bit_colour_has_qt_s_pixels_on_the_gpu() {
    gpu(&shadow(WHITE, "\"#4d000000\"", false));
}

#[test]
fn a_rectangular_shadow_of_eight_bit_colour_has_qt_s_pixels_on_the_cpu() {
    cpu(&shadow(WHITE, "\"#4d000000\"", false));
}

#[test]
fn a_rectangular_shadow_of_three_tenths_alpha_has_qt_s_pixels_on_the_gpu() {
    gpu(&shadow(WHITE, "Qt.rgba(0, 0, 0, 0.3)", false));
}

#[test]
fn a_rectangular_shadow_of_three_tenths_alpha_has_qt_s_pixels_on_the_cpu() {
    cpu(&shadow(WHITE, "Qt.rgba(0, 0, 0, 0.3)", false));
}

#[test]
fn a_rectangular_shadow_of_three_tenths_alpha_has_qt_s_pixels_on_stripes_on_the_gpu() {
    gpu(&shadow(STRIPES, "Qt.rgba(0, 0, 0, 0.3)", false));
}

#[test]
fn a_rectangular_shadow_of_three_tenths_alpha_has_qt_s_pixels_on_stripes_on_the_cpu() {
    cpu(&shadow(STRIPES, "Qt.rgba(0, 0, 0, 0.3)", false));
}

#[test]
fn a_cached_rectangular_shadow_of_three_tenths_alpha_has_qt_s_pixels_on_the_gpu() {
    gpu(&shadow(WHITE, "Qt.rgba(0, 0, 0, 0.3)", true));
}

#[test]
fn a_cached_rectangular_shadow_of_three_tenths_alpha_has_qt_s_pixels_on_the_cpu() {
    cpu(&shadow(WHITE, "Qt.rgba(0, 0, 0, 0.3)", true));
}

#[test]
fn a_rectangular_shadow_of_fractional_channels_has_qt_s_pixels_on_the_gpu() {
    gpu(&shadow(WHITE, "Qt.rgba(0.21, 0.502, 0.698, 0.45)", false));
}

#[test]
fn a_rectangular_shadow_of_fractional_channels_has_qt_s_pixels_on_the_cpu() {
    cpu(&shadow(WHITE, "Qt.rgba(0.21, 0.502, 0.698, 0.45)", false));
}
