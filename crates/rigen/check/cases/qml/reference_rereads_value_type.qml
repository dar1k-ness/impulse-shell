// A value type read from a property (`point`, `font`, `rect`, `color`) re-reads the property
// each time it is used (Qt 6.12, QQmlValueTypeWrapper, a QV4::ReferenceObject); it does not
// hold what was read. So it follows the property through an alias, after the property's
// binding is evaluated again, when it was returned from a function or taken in a signal
// handler and used later, for a property of a Qt type (`Text.font`, `Text.color`) and for one
// that cannot be written (`childrenRect`: a field write is not kept). A field write through it
// removes the binding. A signal argument is a detached copy; a `var` property holds a copy,
// a field write into that copy sends no signal, and a variable read from the `var` property is
// that stored copy, not a reference to the `var` property. `===`, `Object.is`, `Set.has` and
// `indexOf` compare by value. Once the owner is destroyed a field reads undefined and a write
// does nothing. Each step: the signals of st.p, ap, bp, pv, tx.font, then its value.
// qt: 00000,[true,true,true,true,true,false];00000,[true,true,true,true,true,0,false];00000,[null,null,null];11000,[8,8];11000,[3,3];11000,[11,11,11];00000,[true,true,true,true,true,true,true];00100,[5,5];00100,[5,7,5,7];11000,[20,true];11000,[21,21];11000,[31,30];11000,[33,32];33000,[41,42,42];11010,[42,50];00000,[51,50];00000,[52,52,true,true];00010,[52,53];11000,[60,60,60];00001,[20,true,true];00001,[22,22];00002,[24,true,24,true];00000,[10,50,50];00000,[50,60,60];00000,[0,1,true,true];00000,[1,1];00000,[1,true];00000,[1,1];00000,[1,2];00000,[1,false];00000,[true,null,null];00000,[null];end
import QtQuick
Item {
    id: root
    property string result: ""
    property int n: 1
    property QtObject st: QtObject {
        id: sto
        property point p: Qt.point(1, 2)
        property point p2: Qt.point(1, 2)
        property color col: "#ff0000"
    }
    property alias ap: sto.p
    property point bp: Qt.point(n, 0)
    property var pv
    property var eqb: [st.p === st.p, st.p === st.p2, Object.is(st.p, st.p), Object.is(st.p, st.p2), st.p === Qt.point(1, 2), st.p === Qt.point(1, 3)]
    property var box: ({})
    property var dyn
    property Text tx: Text {}
    property Item holder: Item { Item { id: child; width: 10; height: 10 } }
    signal sig(var v)
    signal sigp(point v)
    onSig: v => { box.v = v }
    onSigp: v => { box.v = v }
    Component {
        id: comp
        QtObject {
            property point p: Qt.point(1, 2)
        }
    }
    property int np: 0
    property int nap: 0
    onApChanged: nap++
    property int nbp: 0
    onBpChanged: nbp++
    property int npv: 0
    onPvChanged: npv++
    property int nfont: 0
    Connections {
        target: root.st
        function onPChanged() { root.np++; root.box.h = root.st.p }
    }
    Connections {
        target: root.tx
        function onFontChanged() { root.nfont++ }
    }
    function counts() { return [np, nap, nbp, npv, nfont] }
    function getp() { return st.p }
    function getp2() { var x = st.p; return x }
    function step(f) {
        var c = counts()
        var r
        try { r = f() } catch (e) { r = "E:" + e }
        result += counts().map((v, i) => v - c[i]).join("") + "," + JSON.stringify(r) + ";"
    }
    Component.onCompleted: {
        step(() => { return eqb })
        step(() => { var x = st.p; var y = st.p2; var s = new Set(); s.add(x); return [x === y, Object.is(x, y), Object.is(st.p, st.p2), s.has(y), s.has(st.p), [x].indexOf(y), x === Qt.point(1, 3)] })
        step(() => { var x = st.p; var y = st.p; x.foo = 1; return [x.foo, y.foo, st.p.foo] })
        step(() => { var x = st.p; st.p = Qt.point(8, 9); return [x.x, st.p.x] })
        step(() => { var x = ap; st.p = Qt.point(3, 4); return [x.x, ap.x] })
        step(() => { var x = ap; x.x = 11; return [x.x, st.p.x, ap.x] })
        step(() => { var x = ap; var y = st.p; return [x === y, x == y, ap === ap, ap == ap, st.p === st.p, st.p == st.p, x === x] })
        step(() => { var x = bp; n = 5; return [x.x, bp.x] })
        step(() => { var x = bp; x.y = 7; n = 6; return [x.x, x.y, bp.x, bp.y] })
        step(() => { var x = getp(); st.p = Qt.point(20, 0); return [x.x, getp() === getp()] })
        step(() => { var x = getp2(); x.x = 21; return [x.x, st.p.x] })
        step(() => { sig(st.p); var k = box.v; st.p = Qt.point(30, 0); k.x = 31; return [k.x, st.p.x] })
        step(() => { sigp(st.p); var k = box.v; st.p = Qt.point(32, 0); k.x = 33; return [k.x, st.p.x] })
        step(() => { st.p = Qt.point(40, 0); var h = box.h; st.p = Qt.point(41, 0); var a = h.x; h.x = 42; return [a, h.x, st.p.x] })
        step(() => { pv = st.p; st.p = Qt.point(50, 0); return [pv.x, st.p.x] })
        step(() => { pv.x = 51; return [pv.x, st.p.x] })
        step(() => { var a = pv; a.x = 52; return [pv.x, a.x, a === pv, pv === pv] })
        step(() => { var a = pv; pv = Qt.point(53, 0); return [a.x, pv.x] })
        step(() => { box.r = st.p; var arr = [st.p]; var f = () => box.r; st.p = Qt.point(60, 0); return [box.r.x, arr[0].x, f().x] })
        step(() => { var f = tx.font; tx.font.pixelSize = 20; return [f.pixelSize, tx.font === tx.font, f === f] })
        step(() => { var f = tx.font; f.pixelSize = 22; return [f.pixelSize, tx.font.pixelSize] })
        step(() => { var f = tx.font; var g = tx.font; f.pixelSize = 24; g.bold = true; return [g.pixelSize, f.bold, tx.font.pixelSize, tx.font.bold] })
        step(() => { var r = holder.childrenRect; var w = r.width; child.width = 50; return [w, r.width, holder.childrenRect.width] })
        step(() => { var r = holder.childrenRect; r.width = 5; var w = r.width; child.width = 60; return [w, r.width, holder.childrenRect.width] })
        step(() => { var c = st.col; st.col = "#0000ff"; return [c.r, c.b, st.col === st.col, st.col == st.col] })
        step(() => { var c = st.col; c.g = 1; return [c.g, st.col.g] })
        step(() => { var c = tx.color; tx.color = "#00ff00"; return [c.g, tx.color === tx.color] })
        step(() => { var c = tx.color; c.b = 1; return [c.b, tx.color.b] })
        dyn = comp.createObject(root)
        step(() => { box.dp = dyn.p; return [box.dp.x, box.dp.y] })
        Qt.callLater(phase2)
    }
    function phase2() {
        step(() => { dyn.destroy(); return [box.dp.x, dyn === null] })
        Qt.callLater(phase3)
    }
    function phase3() {
        step(() => { return [dyn === null, box.dp.x, box.dp.y] })
        step(() => { box.dp.x = 5; return [box.dp.x] })
        result += "end"
    }
}
