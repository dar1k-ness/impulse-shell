// qt: shift0=14 y=0 | vis0=false | shift1=0 y=14 | vis1=true
import QtQuick
Item {
    id: root
    property int cur: 0
    property var log: []
    property string result: ""
    Repeater {
        model: 3
        Item {
            id: d
            required property int index
            property bool shown: index === root.cur
            property real shift: shown ? 0 : 14
            visible: shown
            property real ty: d.shift
            onShiftChanged: root.log.push("shift" + index + "=" + shift + " y=" + d.ty)
            onVisibleChanged: root.log.push("vis" + index + "=" + visible)
        }
    }
    Component.onCompleted: { log = []; cur = 1; result = log.join(" | ") }
}
