// qt: a(200) b(200) c(200) | load | c(201) b(201) a(201) | write | m(201) | insert | m(202) c(202) b(202) a(202) | write2 | f(202) | front | f(203) m(203) c(203) b(203) | write3 |
import QtQuick
import QtQml.Models
Item {
    id: root
    width: 200; height: 400
    property int cur: 0
    property string result: ""
    property var seen: []
    function log(s) { seen.push(s) }
    function mark(t) { seen.push("| " + t + " |") }
    ListModel { id: lm; ListElement { name: "a" } ListElement { name: "b" } ListElement { name: "c" } }
    ListView {
        id: lv
        anchors.fill: parent
        model: lm
        delegate: Item {
            width: 20; height: 20
            required property string name
            property int v: { root.log(name + "(" + root.width + ")"); return root.width }
        }
    }
    Component.onCompleted: { mark("load"); width = 201; mark("write"); lm.insert(1, { name: "m" }); lv.forceLayout(); mark("insert"); width = 202; mark("write2"); lm.remove(0); lm.insert(0, { name: "f" }); lv.forceLayout(); mark("front"); width = 203; mark("write3"); result = seen.join(" ") }
}
