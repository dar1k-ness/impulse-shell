// qt: x(a) xChanged y(a) | load | handler(b) x(b) xChanged y(b) | write |
import QtQuick
Item {
    id: root
    property string result: ""
    property var seen: []
    function log(s) { seen.push(s) }
    function mark(t) { seen.push("| " + t + " |") }

    property string p: "a"
    onPChanged: log("handler(" + p + ")")
    Item { id: o1; property string px: { root.log("x(" + root.p + ")"); return root.p }; onPxChanged: root.log("xChanged") }
    Item { id: o2; property string py: { root.log("y(" + o1.px + ")"); return o1.px + root.p } }
    Component.onCompleted: { mark("load"); p = "b"; mark("write"); result = seen.join(" ") }
}
