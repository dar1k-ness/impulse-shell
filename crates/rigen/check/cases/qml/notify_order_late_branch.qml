// qt: y(false) x(a) | load | y(true) | flag | x(b) y(true) | write | y(false) y(true) | again | x(c) y(true) | write2 |
import QtQuick
Item {
    id: root
    property string result: ""
    property var seen: []
    function log(s) { seen.push(s) }
    function mark(t) { seen.push("| " + t + " |") }

    property string p: "a"
    property bool flag: false
    Item { id: o1; property string px: { root.log("x(" + root.p + ")"); return root.p } }
    Item { id: o2; property string py: { root.log("y(" + root.flag + ")"); return root.flag ? root.p : "" } }
    Component.onCompleted: { mark("load"); flag = true; mark("flag"); p = "b"; mark("write"); flag = false; flag = true; mark("again"); p = "c"; mark("write2"); result = seen.join(" ") }
}
