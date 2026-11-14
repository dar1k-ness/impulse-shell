// qt: other dep handler(2) other dep connections2(6) connections1(6) connect-f(6) connect-g(6) | handler(6) other dep connections2(7) connections1(7) connect-g(7) 
import QtQuick
Item {
    id: root
    property string result: ""
    function log(s) { result += s }

    property int p: 1
    property int dep: p + 1
    Connections {
        target: root
        function onPChanged() { root.log("connections1(" + root.dep + ") ") }
    }
    onPChanged: log("handler(" + dep + ") ")
    onDepChanged: log("dep ")
    Connections {
        target: root
        function onPChanged() { root.log("connections2(" + root.dep + ") ") }
    }
    function f() { log("connect-f(" + dep + ") ") }
    function g() { log("connect-g(" + dep + ") ") }
    QtObject {
        id: other
        property int seen: root.p
        onSeenChanged: root.log("other ")
    }

    Component.onCompleted: {
        pChanged.connect(f)
        pChanged.connect(g)
        p = 5
        log("| ")
        pChanged.disconnect(f)
        p = 6
    }
}
