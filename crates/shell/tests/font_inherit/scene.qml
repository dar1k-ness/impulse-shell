import QtQuick
import QtQuick.Controls

// The width of a text follows the font its control takes from the control, popup or window it
// is in. What Qt 6.12 (qml6, offscreen, the tools/testenv image) leaves in `result` is in
// ../font_inherit.rs; a width is compared with the width of the same text whose control sets
// the font itself, so the numbers do not depend on the fonts of the machine.
ApplicationWindow {
    id: root
    width: 400
    height: 300
    visible: true
    font.pixelSize: 11
    property string result: [
        same(inPane, set24), same(labelInPane, label24), same(fieldInPane, field24),
        same(deep, set24bold), same(inWindow, set11), same(textInPane, text),
        same(moved, set24)
    ].join(" ")
    function same(a, b) { return a.implicitWidth > 0 && a.implicitWidth === b.implicitWidth }

    Button { id: set24; text: "Width of text"; font.pixelSize: 24 }
    Button { id: set24bold; text: "Width of text"; font.pixelSize: 24; font.bold: true }
    Button { id: set11; text: "Width of text"; font.pixelSize: 11 }
    Label { id: label24; text: "Width of text"; font.pixelSize: 24 }
    TextField { id: field24; placeholderText: "Width of text"; font.pixelSize: 24 }
    Text { id: text; text: "Width of text" }
    Button { id: inWindow; text: "Width of text" }
    Button { id: moved; text: "Width of text"; Component.onCompleted: parent = inner }
    Pane {
        id: pane
        font.pixelSize: 24
        Button { id: inPane; text: "Width of text" }
        Label { id: labelInPane; text: "Width of text" }
        TextField { id: fieldInPane; placeholderText: "Width of text" }
        Text { id: textInPane; text: "Width of text" }
        Item {
            id: inner
            Frame {
                font.bold: true
                Button { id: deep; text: "Width of text" }
            }
        }
    }
}
