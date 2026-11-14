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

const SCALES: [f64; 4] = [1.0, 1.25, 1.5, 2.0];

fn like_qt_at_scales(body: &str) {
    qt_window::like_qt_by(qt_window::Scale::Output, body, (200, 80), &SCALES, 0, 0);
}

/// The text of a message in the left panel (sidebarLeft/aiChat/MessageTextBlock.qml).
const MESSAGE: &str = "renderType: Text.NativeRendering; color: \"white\"; font.pixelSize: 14; \
    font.hintingPreference: Font.PreferNoHinting";

/// Glyphs whose place in the glyph cache is half of its 64th of a pixel ("l" at 1.25: 1074
/// 64ths times 1.25): QFixed::fromReal cuts the half off, so the glyph is the one drawn a
/// quarter of a pixel to the left.
#[test]
fn glyph_at_half_a_64th() {
    like_qt_at_scales(&format!(
        "Text {{ x: 10; y: 10; text: \"Volume mixer\"; font.family: \"monospace\"; {MESSAGE} }}"
    ));
}

/// A TextArea's text is in nodes of its own, one per paragraph, moved by the paddings: the
/// glyphs are put on device pixels within the node, not within the item, and the nodes are
/// under the clip node of the TextArea, their batch root.
///
/// The second area at y 3 instead of 4 has the tie of `tray_clock` at 1.5: the "w" of "word"
/// comes out a device pixel taller than its texels, an even number, and 6 pixels of its middle
/// row are the texel above in Qt's right triangle and the texel below everywhere in ours.
#[test]
fn text_area() {
    like_qt_at_scales(&format!(
        "import QtQuick.Controls\n\
         TextArea {{ x: 10; y: 10; text: \"echo '[ wavy jig\\nsecond line\"; {MESSAGE} }}"
    ));
    like_qt_at_scales(&format!(
        "import QtQuick.Controls\n\
         TextArea {{ x: 11; y: 4; width: 150; wrapMode: TextEdit.Wrap; \
           textFormat: TextEdit.MarkdownText; \
           text: \"A **bold** word and a line that wraps\\n\\nThe next paragraph\"; {MESSAGE} }}"
    ));
    like_qt_at_scales(&format!(
        "import QtQuick.Controls\n\
         TextArea {{ x: 13; y: 11; padding: 5; text: \"echo '[ wavy jig\"; \
           font.family: \"monospace\"; {MESSAGE} }}"
    ));
}

/// The placeholder of a TextField is a Text item at the field's paddings ("Enter password"
/// on the lock screen, lock/LockSurface.qml).
#[test]
fn placeholder() {
    like_qt_at_scales(&format!(
        "import QtQuick.Controls\n\
         TextField {{ x: 10; y: 10; width: 150; background: null; \
           placeholderText: \"Enter password\"; placeholderTextColor: \"#a0a0a0\"; {MESSAGE} }}"
    ));
}

/// A code block of the left panel's guide: lines of text under an item that clips (the
/// Flickable), whose clip node is the batch root of the text nodes. The node's place is then
/// two float sums, and at 1.5 a line that lands on half a pixel goes the other way than under
/// the window's root.
#[test]
fn text_under_a_clip() {
    like_qt_at_scales(
        "Item { x: 15; y: 3; width: 180; height: 74\n\
         Flickable { anchors.fill: parent; anchors.margins: 10; contentHeight: 200; clip: true\n\
         Rectangle { width: parent.width; height: 90; color: \"#202020\"; radius: 8\n\
         Rectangle { anchors.fill: parent; anchors.margins: 14; color: \"#303030\"; radius: 4\n\
         Text { anchors.fill: parent; anchors.margins: 10; color: \"white\"\n\
         renderType: Text.NativeRendering; verticalAlignment: Text.AlignVCenter\n\
         font.hintingPreference: Font.PreferDefaultHinting; font.family: \"monospace\"\n\
         font.pixelSize: 12; wrapMode: Text.WrapAnywhere\n\
         text: \"#!/bin/sh\\necho '[\\n  {\\\"icon\\\": \\\"terminal\\\"\" } } } } }",
    );
}

/// Text under an item that clips, with the item and the text at each quarter of a pixel
/// (1.25) and each half (1.5) along x and y: the vertex shader puts the text on whole pixels
/// of its batch root, the clip node, which itself is not on one. At 1.5 half of the items are
/// at half a pixel, so the centre of every pixel of their text is exactly on the edge of two
/// texels of the glyph. We take the texel Qt takes in all those columns but one, 12 pixels
/// high: there the choice is the last bit of a float sum over the glyph's place in Qt's
/// atlas, which is not ours.
#[test]
fn text_under_a_clip_at_parts_of_a_pixel() {
    let cells: String = (0..16)
        .map(|i| {
            let (a, c) = (i % 4, i / 4);
            let (x, y) = (4 + (i % 8) * 56 + a, 4 + (i / 8) * 36 + c);
            format!(
                "Item {{ x: {x}; y: {y}; width: 48; height: 26; clip: true\n\
                 Text {{ x: {c}; y: {a}; color: \"white\"; text: \"il1{{\"\n\
                 renderType: Text.NativeRendering; font.family: \"monospace\"\n\
                 font.hintingPreference: Font.PreferDefaultHinting; font.pixelSize: 12 }} }}\n"
            )
        })
        .collect();
    qt_window::like_qt_by(qt_window::Scale::Output, &cells, (456, 80), &SCALES, 0, 12);
}

/// A layer whose texture is not a whole number of texels per pixel (181 * 1.25 = 226.25):
/// Qt snaps a glyph to the grid of the layer's own ratio, the larger of its texels to the
/// unit along x and y (QSGRhiLayer::grab), and each axis' scale comes after, in the
/// projection. The mask stretches the texture back, so a level of 255 may differ.
#[test]
fn text_in_a_layer_between_texels() {
    qt_window::like_qt_by(
        qt_window::Scale::Output,
        "import Qt5Compat.GraphicalEffects\n\
         Item { id: sw; x: 15; y: 1; width: 181; height: 76; clip: true; layer.enabled: true\n\
         layer.effect: OpacityMask { maskSource: Rectangle { width: sw.width; height: sw.height; radius: 8 } }\n\
         Flickable { anchors.fill: parent; anchors.margins: 10; contentHeight: 200; clip: true\n\
         Rectangle { width: parent.width; height: 90; color: \"#202020\"; radius: 8\n\
         Rectangle { anchors.fill: parent; anchors.margins: 14; color: \"#303030\"; radius: 4\n\
         Text { anchors.fill: parent; anchors.margins: 10; color: \"white\"\n\
         renderType: Text.NativeRendering; verticalAlignment: Text.AlignVCenter\n\
         font.hintingPreference: Font.PreferDefaultHinting; font.family: \"monospace\"\n\
         font.pixelSize: 12; wrapMode: Text.WrapAnywhere\n\
         text: \"#!/bin/sh\\necho '[\\n  {\\\"icon\\\": \\\"terminal\\\"\" } } } } }",
        (200, 80),
        &SCALES,
        1,
        0,
    );
}

/// Layers of sizes that are not a whole number of texels along x, along y or both, with
/// text at several places in them: the grid a glyph is snapped to is one for both axes, so
/// the height of a layer moves its glyphs along x. Without the size 66 x 31: at 1.5 it is
/// 99 x 47 texels, a glyph is drawn 93 / 94 as wide, and the centre of column 46 is exactly
/// on the edge of two texels of a glyph (46.5 * 94 / 93 = 47). Qt takes the other texel
/// there: 17 pixels in that one column, see `text_under_a_clip_at_parts_of_a_pixel`.
#[test]
fn text_in_layers_of_part_sizes() {
    let cells: String = (0..12)
        .filter(|i| *i != 5)
        .map(|i| {
            let (w, h, m, c) = (61 + i, 31 + i % 5, 2 + i % 4, i % 3);
            let (x, y) = (2 + (i % 6) * 76, 2 + (i / 6) * 38);
            format!(
                "Rectangle {{ id: s{i}; x: {x}; y: {y}; width: {w}; height: {h}\n\
                 color: \"#303030\"; layer.enabled: true\n\
                 layer.effect: OpacityMask {{ maskSource: Rectangle {{\n\
                 width: s{i}.width; height: s{i}.height; radius: 4 }} }}\n\
                 Item {{ anchors.fill: parent; anchors.margins: {m}; clip: {clip}\n\
                 Text {{ x: {c}; y: {c}; color: \"white\"; text: \"il1{{H\"\n\
                 renderType: Text.NativeRendering; font.family: \"monospace\"\n\
                 font.hintingPreference: Font.PreferDefaultHinting; font.pixelSize: 12 }} }} }}\n",
                clip = i % 2 == 0
            )
        })
        .collect();
    qt_window::like_qt_by(
        qt_window::Scale::Output,
        &format!("import Qt5Compat.GraphicalEffects\n{cells}"),
        (460, 80),
        &SCALES,
        1,
        0,
    );
}
