// A binding that gave `undefined` reset its property; when it gives a value again the property
// is a set one again, as after an assignment: it no longer follows the wider one.
// qt: 2,3,4,3,1,6,6,2,2,2,2 | 5,5,5,12,6,-1,-1,5,5,5,5 | 2,3,4,3,1,6,6,2,2,2,2 | 2,3,4,3,1,6,6,2,2,2,2
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
Item {
    id: root
    // 0: the bindings give a value, 1: `undefined`, 2: the value again.
    property int step: 0
    Rectangle { id: rect; radius: 5; topLeftRadius: root.step == 1 ? undefined : 2 }
    Control { id: control; padding: 5; leftPadding: root.step == 1 ? undefined : 3; verticalPadding: root.step == 1 ? undefined : 4 }
    Pane { id: pane; horizontalPadding: root.step == 1 ? undefined : 3 }
    Popup { id: popup; margins: 6; leftMargin: root.step == 1 ? undefined : 1 }
    Popup { id: wide; margins: root.step == 1 ? undefined : 6 }
    Item { id: anchored; anchors.margins: 5; anchors.leftMargin: root.step == 1 ? undefined : 2 }
    RowLayout { Item { id: laid; Layout.margins: 5; Layout.leftMargin: root.step == 1 ? undefined : 2 } }
    Column { id: column; padding: 5; leftPadding: root.step == 1 ? undefined : 2 }
    Text { id: text; padding: 5; leftPadding: root.step == 1 ? undefined : 2 }
    function row() {
        return [rect.topLeftRadius, control.leftPadding, control.topPadding, pane.leftPadding,
                popup.leftMargin, wide.margins, wide.leftMargin, anchored.anchors.leftMargin,
                laid.Layout.leftMargin, column.leftPadding, text.leftPadding].join(",")
    }
    property string result
    Component.onCompleted: {
        var l = [row()]
        step = 1; l.push(row())
        step = 2; l.push(row())
        // The wider one changes: a set narrower one keeps its value.
        rect.radius = 7; control.padding = 7; popup.margins = 7; anchored.anchors.margins = 7
        laid.Layout.margins = 7; column.padding = 7; text.padding = 7
        l.push(row())
        result = l.join(" | ")
    }
}
