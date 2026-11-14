//! `Text.NativeRendering` on a monitor whose scale is not 1, against Qt itself
//! (support/qt_window.rs, the scale set on the compositor's output as on a real monitor), with
//! no tolerance: every device pixel is Qt's.

#[path = "support/qt_window.rs"]
mod qt_window;

const DPRS: [f64; 3] = [3.0, 2.0, 1.5];

/// The text of the Waffle panel (qml/modules/waffle/looks/WText.qml).
const WTEXT: &str = "renderType: Text.NativeRendering; verticalAlignment: Text.AlignVCenter; \
    color: \"#ffffff\"; font.family: \"Noto Sans\"; font.pixelSize: 11; \
    font.weight: Font.Medium; font.variableAxes: ({ \"wdth\": 25 })";

fn like_qt(body: &str, dprs: &[f64]) {
    qt_window::like_qt_by(qt_window::Scale::Output, body, (160, 64), dprs, 0, 0);
}

/// The clock and date of the Waffle tray (qml/modules/waffle/bar/TimeButton.qml).
///
/// Not at 1.5: the texts land on odd logical pixels, so every edge of a glyph's quad is half a
/// device pixel and the vertex shader's floats decide which way each one goes. The "2" of the
/// date comes out 7 device pixels wide for its 8 texels; the centre of its fourth column is
/// then exactly the edge between two texels, and which of them the nearest filter takes is the
/// rasterizer's rounding: 3 pixels of that column are not Qt's.
#[test]
fn tray_clock() {
    like_qt(
        &format!(
            "Item {{ x: 20; y: 6; width: 95; height: 48\n\
         Row {{ anchors.centerIn: parent; spacing: 7\n\
         Column {{ anchors.verticalCenter: parent.verticalCenter\n\
         Text {{ anchors.right: parent.right; text: \"12:34\"; {WTEXT} }}\n\
         Text {{ anchors.right: parent.right; text: \"15/01/2026\"; {WTEXT} }} }} }} }}"
        ),
        &DPRS[..2],
    );
}

/// The same two texts at 1.5 on even logical pixels, where the quads' edges are whole device
/// pixels.
#[test]
fn tray_text_at_one_and_a_half() {
    like_qt(
        &format!(
            "Text {{ x: 20; y: 6; text: \"12:34\"; {WTEXT} }}\n\
         Text {{ x: 20; y: 30; text: \"15/01/2026\"; {WTEXT} }}"
        ),
        &DPRS[2..],
    );
}

/// Items between whole logical pixels: the glyphs' subpixel offsets are the line's own, and
/// the item's position moves whole device pixels.
#[test]
fn text_between_pixels() {
    like_qt(
        "Text { x: 6.5; y: 2.25; text: \"Hamburg 12:34\"; renderType: Text.NativeRendering; color: \"white\"; font.pixelSize: 13 }\n\
         Item { x: 0.3; y: 20.7; Text { x: 80.4; y: 0.1; text: \"wavy jig\"; renderType: Text.NativeRendering; color: \"white\"; font.pixelSize: 16; font.bold: true } }\n\
         Text { x: 5.75; y: 40.5; width: 100.5; horizontalAlignment: Text.AlignHCenter; text: \"15/01/2026\"; renderType: Text.NativeRendering; color: \"#80ffffff\"; font.pixelSize: 11; font.italic: true }",
        &DPRS,
    );
}
