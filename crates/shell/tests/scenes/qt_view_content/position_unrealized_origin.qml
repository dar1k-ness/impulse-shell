// qt: call y 2400 oy -1600 ch 5000
import QtQuick
// positionViewAtIndex on a row that has no delegate yet (position_unrealized.qml): `originY` and
// `contentHeight` read in the handler that made the call are already those of the new rows.
Item {
    id: root
    width: 200; height: 200
    property string result: ""
    property var out: []
    function snap(t) { out.push(t + " y " + f.contentY + " oy " + f.originY + " ch " + f.contentHeight) }
    ListView {
        id: f
        width: 200; height: 200
        model: 100
        delegate: Rectangle { required property int index; width: 200; height: index < 20 ? 30 : 50 }
    }
    Timer { interval: 100; running: true; onTriggered: { f.positionViewAtIndex(80, ListView.Beginning); root.snap("call"); root.result = root.out.join(" | ") } }
}
