// A padding or a popup margin nothing wrote reads what the style gives the fresh instance while
// no wider one was written either; with a wider one written, or after a reset, it follows that.
// qt: 0 0 0 0 6 6 -1 -1 9 9 6 8 5 3 2 6 6 7 0 0 -1 -1
import QtQuick
import QtQuick.Controls
Item {
    id: root
    ComboBox { id: combo }
    Menu { id: menu }
    ToolTip { id: tip }
    Popup { id: popup }
    Popup { id: wide; margins: 9 }
    Button { id: button }
    Button { id: padded; padding: 3 }
    Button { id: sides; horizontalPadding: 2 }
    Control { id: control; padding: 7; topPadding: 3 }
    property string result
    Component.onCompleted: {
        var out = [combo.padding, combo.leftPadding, menu.margins, menu.leftMargin,
            tip.margins, tip.topMargin, popup.margins, popup.rightMargin, wide.margins, wide.bottomMargin,
            button.padding, button.leftPadding, padded.leftPadding, padded.topPadding,
            sides.leftPadding, sides.topPadding, sides.padding]
        control.topPadding = undefined
        out.push(control.topPadding)
        control.padding = undefined
        out.push(control.topPadding, control.padding)
        tip.margins = undefined
        out.push(tip.margins, tip.leftMargin)
        result = out.join(" ")
        console.log(result)
    }
}
