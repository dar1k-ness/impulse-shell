// qt: before y 0 oy 0 ch 3000 | call y 2400 | later y 2400 oy -1600 ch 5000 | rest y 2400 oy -1600 ch 5000
import QtQuick
// positionViewAtIndex on a row that has no delegate yet: 20 rows of 30, then 80 of 50. The view
// goes where the average size puts the row and stays there; its origin moves instead.
// What `originY` and `contentHeight` are inside the handler that made the call is in
// position_unrealized_origin.qml.
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
    Timer { interval: 100; running: true; onTriggered: { root.snap("before"); f.positionViewAtIndex(80, ListView.Beginning); root.out.push("call y " + f.contentY) } }
    Timer { interval: 200; running: true; onTriggered: root.snap("later") }
    Timer { interval: 900; running: true; onTriggered: { root.snap("rest"); root.result = root.out.join(" | ") } }
}
