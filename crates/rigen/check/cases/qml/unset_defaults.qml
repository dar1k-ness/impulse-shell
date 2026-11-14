// A property of a Qt type that nothing sets reads the value Qt gives a fresh instance, not the
// zero of its C++ type; a field of an unset `font` reads the application font's; a property
// written and then reset by `undefined` reads that value again.
// qt: 4 true 1 source true 1 true 3 2 1 2147483647 #0000ff Sans Serif 20 15 400 false false true 12 9 ● true #000080 true 4 128 1 800 true 3 1500 320 -1 2 -1 100 150 -1 250 1 0 1.70158 -1 200 * 2 300 24 #00000000 11 true true 0 1 true -1 17 6 -1 7 4 2 700 -1
import QtQuick
import QtQuick.Controls
Item {
    id: root
    Item { id: item }
    Rectangle { id: rect }
    Text { id: text; font.pointSize: 15 }
    TextInput { id: input }
    Image { id: image }
    MouseArea { id: area }
    Flickable { id: flick }
    ListView { id: list }
    GridView { id: grid }
    Grid { id: cells }
    NumberAnimation { id: anim }
    SmoothedAnimation { id: smooth }
    Transition { id: trans }
    Button { id: button }
    RadioButton { id: radio }
    ItemDelegate { id: row }
    Slider { id: slider }
    Popup { id: popup }
    ToolTip { id: tip }
    property string result
    Component.onCompleted: {
        var before = [item.transformOrigin, item.smooth, item.layer.format, item.layer.samplerName,
            item.anchors.alignWhenCentered, rect.border.width, rect.border.pixelAligned,
            text.elide, text.textFormat, text.lineHeight, text.maximumLineCount, text.linkColor,
            text.font.family, text.font.pixelSize, text.font.pointSize, text.font.weight,
            text.font.bold, text.font.italic, text.font.kerning,
            input.font.pixelSize, input.font.pointSize, input.passwordCharacter, input.autoScroll,
            input.selectionColor, image.cache, image.horizontalAlignment, image.verticalAlignment,
            area.acceptedButtons, area.pressAndHoldInterval, flick.interactive, flick.boundsBehavior,
            flick.flickDeceleration, list.cacheBuffer, list.currentIndex, list.orientation,
            list.highlightMoveDuration, grid.cellWidth, grid.highlightMoveDuration, cells.columns,
            anim.duration, anim.loops, anim.easing.type, anim.easing.overshoot, smooth.duration,
            smooth.velocity, trans.from, button.display, button.autoRepeatDelay, button.icon.width,
            button.icon.color, button.focusPolicy, radio.checkable, radio.autoExclusive,
            row.focusPolicy, slider.orientation, slider.live, popup.margins, popup.closePolicy,
            tip.margins, tip.timeout]
        list.cacheBuffer = 7
        rect.border.width = 4
        area.acceptedButtons = Qt.RightButton
        text.font.bold = true
        var set = [list.cacheBuffer, rect.border.width, area.acceptedButtons, text.font.weight]
        popup.margins = 3
        popup.margins = undefined
        result = before.concat(set, [popup.margins]).map(String).join(" ")
    }
}
