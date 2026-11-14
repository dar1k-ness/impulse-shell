// qt: Z(n1) A(n1) | load | B(n1) | made | B(n2) A(n2) Z(n2) | write |
import QtQuick
Item {
    id: root
    property string result: ""
    property var seen: []
    function log(s) { seen.push(s) }
    function mark(t) { seen.push("| " + t + " |") }

    objectName: "n1"
    Item { id: a; property string v: { root.log("A(" + root.objectName + ")"); return root.objectName } }
    Component { id: comp; Item { property string v: { root.log("B(" + root.objectName + ")"); return root.objectName } } }
    Item { id: z; property string v: { root.log("Z(" + root.objectName + ")"); return root.objectName } }
    Component.onCompleted: { mark("load"); comp.createObject(root); mark("made"); objectName = "n2"; mark("write"); result = seen.join(" ") }
}
