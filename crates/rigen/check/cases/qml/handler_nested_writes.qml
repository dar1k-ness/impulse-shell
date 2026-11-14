// qt: xs ys zs all(1) p[2,1,1,1 q[2,1,1,1 r(2,1,1,1) zs all(15) 2,1,6,15] ys 2,6,6,15] xs | 6 6 6 15
import QtQuick
Item {
    id: root
    property string result: ""
    function log(s) { result += s }

    property int p: 1
    property int q: 0
    property int r: 0
    property int xs: p + 1
    property int ys: q + 1
    property int zs: r + 1
    property int all: p + q + r
    onPChanged: {
        log("p[" + xs + "," + ys + "," + zs + "," + all + " ")
        q = p
        log(xs + "," + ys + "," + zs + "," + all + "] ")
    }
    onQChanged: {
        log("q[" + xs + "," + ys + "," + zs + "," + all + " ")
        r = q
        log(xs + "," + ys + "," + zs + "," + all + "] ")
    }
    onRChanged: log("r(" + xs + "," + ys + "," + zs + "," + all + ") ")
    onXsChanged: log("xs ")
    onYsChanged: log("ys ")
    onZsChanged: log("zs ")
    onAllChanged: log("all(" + all + ") ")

    Component.onCompleted: {
        p = 5
        log("| " + xs + " " + ys + " " + zs + " " + all)
    }
}
