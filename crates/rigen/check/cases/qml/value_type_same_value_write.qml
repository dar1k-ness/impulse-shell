// A write that leaves a value type property as it was sends no change signal, as in Qt 6.12:
// to a member (`p.x = p.x`), of the property itself (`p = p`, an equal value), through a
// reference (`var t = p; t.x = t.x`) and by destructuring. A `property var` that holds a value
// type sends none for a member write at all, though the member changes, and a `date` compares
// by its time. Each step: the signals of p, r, s, v, q, m, pv and d it sent.
// qt: 00000000;00000000;00000000;00000000;10000000;00000000;00000000;10000000;00000000;20000000;00000000;01000000;00000000;00000000;00010000;00000000;00000000;00000100;00000000;00000000,7;00000010;00000000;00000001;
import QtQuick
Item {
    id: root
    property string result: ""
    property point p: Qt.point(1, 2)
    property int np: 0
    onPChanged: np++
    property rect r: Qt.rect(1, 2, 3, 4)
    property int nr: 0
    onRChanged: nr++
    property size s: Qt.size(3, 4)
    property int ns: 0
    onSChanged: ns++
    property vector3d v: Qt.vector3d(1, 2, 3)
    property int nv: 0
    onVChanged: nv++
    property quaternion q: Qt.quaternion(1, 2, 3, 4)
    property int nq: 0
    onQChanged: nq++
    property matrix4x4 m: Qt.matrix4x4()
    property int nm: 0
    onMChanged: nm++
    property var pv: Qt.point(1, 2)
    property int npv: 0
    onPvChanged: npv++
    property date d: new Date(2020, 1, 1)
    property int nd: 0
    onDChanged: nd++
    function step(f) {
        var n = [np, nr, ns, nv, nq, nm, npv, nd]
        var r = f()
        result += [np, nr, ns, nv, nq, nm, npv, nd].map((c, i) => c - n[i]).join("")
            + (r === undefined ? "" : "," + r) + ";"
    }
    Component.onCompleted: {
        step(() => { p.x = p.x })
        step(() => { p = p })
        step(() => { p = Qt.point(1, 2) })
        step(() => { root.p.x = 1; p.x += 0 })
        step(() => { p.x = 3 })
        step(() => { p.x = "3" })
        step(() => { var t = p; t.x = t.x; t.y = 2 })
        step(() => { var t = p; t.x = 11 })
        step(() => { [p.x] = [11]; ({ y: p.y } = { y: 2 }) })
        step(() => { p.x++; p.x-- })
        step(() => { r.width = r.width; r = r; r = Qt.rect(1, 2, 3, 4); r.x = 1 })
        step(() => { r.x = 2 })
        step(() => { s.width = 3; s = s })
        step(() => { v.x = v.x; v.y = 2; v = v })
        step(() => { v.z = 9 })
        step(() => { q.scalar = 1; q = q })
        step(() => { m.m11 = 1; m = m })
        step(() => { m.m11 = 2 })
        step(() => { pv.x = pv.x; pv = pv; pv = Qt.point(1, 2) })
        step(() => { pv.x = 7; return pv.x })
        step(() => { pv = Qt.point(8, 2) })
        step(() => { d = d; d = new Date(2020, 1, 1) })
        step(() => { d = new Date(2021, 1, 1) })
    }
}
