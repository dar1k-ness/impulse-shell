// qt: w(a) lab(a) | load | w(hh) lab(hh) w(hh) | write |
import QtQuick
Item {
    id: root
    property string result: ""
    property var seen: []
    function log(s) { seen.push(s) }
    function mark(t) { seen.push("| " + t + " |") }

    property string name: "a"
    Item {
        id: lab
        implicitWidth: { root.log("lab(" + root.name + ")"); return root.name.length * 10 }
        Item { id: tab; property real w: { root.log("w(" + root.name + ")"); return lab.implicitWidth + 8 } }
    }
    Component.onCompleted: { mark("load"); name = "hh"; mark("write"); result = seen.join(" ") }
}
