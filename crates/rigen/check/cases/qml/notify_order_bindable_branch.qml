// qt: y(false) x(10) | load | y(true) | flag | y(true) x(20) | write | y(false) y(true) | again | y(true) x(30) | write2 |
import QtQuick
Item {
    id: root
    property string result: ""
    property var seen: []
    function log(s) { seen.push(s) }
    function mark(t) { seen.push("| " + t + " |") }

    width: 10
    property bool flag: false
    Item { id: o1; property real px: { root.log("x(" + root.width + ")"); return root.width } }
    Item { id: o2; property real py: { root.log("y(" + root.flag + ")"); return root.flag ? root.width : 0 } }
    Component.onCompleted: { mark("load"); flag = true; mark("flag"); width = 20; mark("write"); flag = false; flag = true; mark("again"); width = 30; mark("write2"); result = seen.join(" ") }
}
