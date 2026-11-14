// qt: a(0) b(0) c(0) | load | a(1) b(1) c(1) | write | m(1) | insert | a(2) b(2) c(2) m(2) | write2 | f(2) | front | b(3) c(3) m(3) f(3) | write3 |
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
    Repeater {
        model: lm
        Item {
            width: 20; height: 20
            required property string name
            property int v: { root.log(name + "(" + root.cur + ")"); return root.cur }
        }
    }
    Component.onCompleted: { mark("load"); cur = 1; mark("write"); lm.insert(1, { name: "m" }); mark("insert"); cur = 2; mark("write2"); lm.remove(0); lm.insert(0, { name: "f" }); mark("front"); cur = 3; mark("write3"); result = seen.join(" ") }
}
