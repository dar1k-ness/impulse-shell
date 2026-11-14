// qt: onY(11) onBoth(11) onX(2) p[11,11,2 onY(51) onBoth(55) 51,55,2] onX(10) | 51 55 10
import QtQuick
Item {
    id: root
    property string result: ""
    function log(s) { result += s }

    property int p: 1
    property int q: 10
    property int onY: q + 1
    property int onBoth: p + q
    property int onX: p * 2
    onPChanged: {
        log("p[" + onY + "," + onBoth + "," + onX + " ")
        q = p * 10
        log(onY + "," + onBoth + "," + onX + "] ")
    }
    onOnYChanged: log("onY(" + onY + ") ")
    onOnBothChanged: log("onBoth(" + onBoth + ") ")
    onOnXChanged: log("onX(" + onX + ") ")

    Component.onCompleted: {
        p = 5
        log("| " + onY + " " + onBoth + " " + onX)
    }
}
