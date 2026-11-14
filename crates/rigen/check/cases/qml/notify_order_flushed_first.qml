// qt: v w(a) lab(a) | load | w(hh) lab(hh) w(hh) v | write |
import QtQuick
Item {
    id: root
    property string result: ""
    property var seen: []
    function log(s) { seen.push(s) }
    function mark(t) { seen.push("| " + t + " |") }

    property string name: "a"
    property real w: { log("w(" + name + ")"); return lab.implicitWidth + 8 }
    Item { id: lab; implicitWidth: { root.log("lab(" + root.name + ")"); return root.name.length * 10 } }
    Item { id: c; property real v: { root.log("v"); return root.w } }
    Component.onCompleted: { mark("load"); name = "hh"; mark("write"); result = seen.join(" ") }
}
