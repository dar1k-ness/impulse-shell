// qt: z(true) y(true) x(10) | load | z(false) y(false) | flag | x(20) y(false) | write | z(true) y(true) | again | x(30) y(true) z(true) | write2 |
import QtQuick
Item {
    id: root
    property string result: ""
    property var seen: []
    function log(s) { seen.push(s) }
    function mark(t) { seen.push("| " + t + " |") }

    width: 10
    property bool flag: true
    property string p: "a"
    Item { id: o1; property real px: { root.log("x(" + root.width + ")"); return root.width } }
    Item { id: o2; property real py: { root.log("y(" + root.flag + ")"); return root.flag ? root.width : 0 } }
    Item { id: o3; property string pz: { root.log("z(" + root.flag + ")"); return root.flag ? root.p : "" } }
    Component.onCompleted: { mark("load"); flag = false; mark("flag"); width = 20; p = "b"; mark("write"); flag = true; mark("again"); width = 30; p = "c"; mark("write2"); result = seen.join(" ") }
}
