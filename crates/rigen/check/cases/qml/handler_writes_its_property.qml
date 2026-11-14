// qt: bdep(2) bp1[2 1,2] p1[0 p2[0 p3[0 3,0] dep(30) 3,30] 3,30] | 3 30 | bp2[2 bp7[2 7,2] bdep(8) 7,8] | 7 8
import QtQuick
Item {
    id: root
    property string result: ""
    function log(s) { result += s }

    property int p: 0
    property int dep: p * 10
    onPChanged: {
        log("p" + p + "[" + dep + " ")
        if (p < 3)
            p = p + 1
        log(p + "," + dep + "] ")
    }
    onDepChanged: log("dep(" + dep + ") ")

    // The property has a binding: the handler's write takes it away.
    QtObject {
        id: bound
        property int src: 1
        property int p: src
        property int dep: p + 1
        onPChanged: {
            root.log("bp" + p + "[" + dep + " ")
            if (p == 2)
                p = 7
            root.log(p + "," + dep + "] ")
        }
        onDepChanged: root.log("bdep(" + dep + ") ")
    }

    Component.onCompleted: {
        p = 1
        log("| " + p + " " + dep + " | ")
        bound.src = 2
        bound.src = 3
        log("| " + bound.p + " " + bound.dep)
    }
}
