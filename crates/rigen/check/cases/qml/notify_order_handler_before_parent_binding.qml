// qt: | load | c:false b:false a:false ready=true | write |
import QtQuick
Item {
    id: root
    property string result: ""
    property var seen: []
    function log(s) { seen.push(s) }
    function mark(t) { seen.push("| " + t + " |") }
    function look(who) { log(who + ":" + cell.ready) }
    Item {
        id: cell
        readonly property bool ready: a.n == 1 && b.n == 1 && c.n == 1
        onReadyChanged: root.log("ready=" + ready)
        Item { id: a; property int n: 0; onNChanged: root.look("a") }
        Item { id: b; property int n: 0; onNChanged: root.look("b") }
        Item { id: c; property int n: 0; onNChanged: root.look("c") }
    }
    Component.onCompleted: { mark("load"); c.n = 1; b.n = 1; a.n = 1; mark("write"); result = seen.join(" ") }
}
