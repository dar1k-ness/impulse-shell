// qt: b(a=2) a(b=3) last first(3,later=20,child=102,last=122) later(3) own(20) | a(b=3) b(a=6) 
import QtQuick
Item {
    id: root
    property string result: ""
    function log(s) { result += s }

    // While the document is created: a handler reads a property whose binding has not run yet.
    property int src: 2
    property int first: src + 1
    property int later: src * 10
    property int last: child.own + later
    onFirstChanged: log("first(" + first + ",later=" + later + ",child=" + child.own + ",last=" + last + ") ")
    onLaterChanged: log("later(" + first + ") ")
    onLastChanged: log("last ")
    QtObject {
        id: child
        property int own: root.src + 100
        onOwnChanged: root.log("own(" + root.later + ") ")
    }

    // After that: two bindings read one property, each one's handler reads the other.
    QtObject {
        id: pair
        property int p: 1
        property int a: p + 1
        property int b: p + 2
        onAChanged: root.log("a(b=" + b + ") ")
        onBChanged: root.log("b(a=" + a + ") ")
    }

    Component.onCompleted: {
        log("| ")
        pair.p = 5
    }
}
