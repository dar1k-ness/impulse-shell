// A `var` property holds a value type as the object script gave it (Qt 6.12): the one read
// back is that object (`a === pv`), a write to its field changes what the property holds and
// sends no change signal. One read from a property is stored as a copy that no longer follows
// it, as is the one a `var` signal argument carries; an object field and an array element
// keep the reference.
// qt: #800000|#800000|true|true|1|#800000|#0000ff|2|#ff0000|#ff0000|#ffff00|#ffff00|#008000|3|5|true|#008000|#008000|#ff8000|#ff8000|#0000ff|#010203|#010203
import QtQuick
Item {
    id: root
    property string result
    property var pv
    property var pp
    property color col: "red"
    property int n: 0
    onPvChanged: n++
    signal sig(var v)
    property var got
    onSig: v => { got = v }
    Component.onCompleted: {
        var l = []
        pv = Qt.rgba(1, 0, 0, 1)
        var a = pv; a.r = 0.5; l.push("" + a, "" + pv, a === pv, pv === pv, n)
        var b = pv; pv = Qt.rgba(0, 0, 1, 1); l.push("" + b, "" + pv, n)
        pv = col; var c = pv; col = "green"; l.push("" + c, "" + pv)
        c.g = 1; l.push("" + c, "" + pv, "" + col, n)
        pp = Qt.point(1, 2); var d = pp; d.x = 5; l.push(pp.x, d === pp)
        sig(col); var e = got; col = "blue"; l.push("" + e, "" + got); e.r = 1; l.push("" + e, "" + got, "" + col)
        var o = { c: col }; var arr = [col]; col = "#010203"; l.push("" + o.c, "" + arr[0])
        result = l.join("|")
    }
}
