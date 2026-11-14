// qt: z(true) x(a) | load | z(true) x(b) | write | z(false) | flag | x(c) z(false) | write2 |
import QtQuick
Item {
    id: root
    property string result: ""
    property var seen: []
    function log(s) { seen.push(s) }
    function mark(t) { seen.push("| " + t + " |") }

    property string p: "a"
    property string q: "1"
    property bool flag: true
    Item { id: o1; property string px: { root.log("x(" + root.p + ")"); return root.p } }
    Item { id: o2; property string pz: { root.log("z(" + root.flag + ")"); return root.flag ? root.p + root.q : root.q + root.p } }
    Component.onCompleted: { mark("load"); p = "b"; mark("write"); flag = false; mark("flag"); p = "c"; mark("write2"); result = seen.join(" ") }
}
