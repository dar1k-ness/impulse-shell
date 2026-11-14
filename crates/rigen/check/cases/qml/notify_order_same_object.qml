// qt: a b c | load | b c | write | a | flag | b c a | write2 |
import QtQuick
Item {
    id: root
    property string result: ""
    property var seen: []
    function log(s) { seen.push(s) }
    function mark(t) { seen.push("| " + t + " |") }

    property string p: "a"
    property bool flag: false
    Item {
        id: o
        property string a: { root.log("a"); return root.flag ? root.p : "" }
        property string b: { root.log("b"); return root.p }
        property string c: { root.log("c"); return root.p }
    }
    Component.onCompleted: { mark("load"); p = "b"; mark("write"); flag = true; mark("flag"); p = "c"; mark("write2"); result = seen.join(" ") }
}
