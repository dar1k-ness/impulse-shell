// qt: Z(a) A(a) | load | B(a) | made | Z(b) A(b) B(b) | write |
import QtQuick
Item {
    id: root
    property string result: ""
    property var seen: []
    function log(s) { seen.push(s) }
    function mark(t) { seen.push("| " + t + " |") }

    property string p: "a"
    Item { id: a; property string px: { root.log("A(" + root.p + ")"); return root.p } }
    Component { id: comp; Item { property string px: { root.log("B(" + root.p + ")"); return root.p } } }
    Item { id: z; property string px: { root.log("Z(" + root.p + ")"); return root.p } }
    Component.onCompleted: { mark("load"); comp.createObject(root); mark("made"); p = "b"; mark("write"); result = seen.join(" ") }
}
