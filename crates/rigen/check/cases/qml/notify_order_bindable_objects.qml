// qt: Z(10) A(10) | load | B(10) | made | B(20) A(20) Z(20) | write |
import QtQuick
Item {
    id: root
    property string result: ""
    property var seen: []
    function log(s) { seen.push(s) }
    function mark(t) { seen.push("| " + t + " |") }

    width: 10
    Item { id: a; property real v: { root.log("A(" + root.width + ")"); return root.width } }
    Component { id: comp; Item { property real v: { root.log("B(" + root.width + ")"); return root.width } } }
    Item { id: z; property real v: { root.log("Z(" + root.width + ")"); return root.width } }
    Component.onCompleted: { mark("load"); comp.createObject(root); mark("made"); width = 20; mark("write"); result = seen.join(" ") }
}
