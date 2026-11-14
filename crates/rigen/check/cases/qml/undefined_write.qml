// `undefined` written to a property: one with a RESET function goes back to what it is when
// nothing is written (`width: undefined` and `width: ;` are the implicit width), a `var`
// property takes it, any other keeps its value; there an assignment throws.
// qt: 0 0 7 9 3 0 1 6 2.5 s undefined 8 Error: Cannot assign [undefined] to int 5 Error: Cannot assign [undefined] to int Error: Cannot assign [undefined] to double 0.25 0 0
import QtQuick
Item {
    id: root
    Item { id: a; width: undefined }
    Item { id: b; width: ;
    }
    Item { id: c; implicitWidth: 7; width: undefined }
    Item { id: d; implicitWidth: 9; width: 5 }
    Item { id: e; property bool on: true; width: on ? 5 : undefined; implicitWidth: 3 }
    Item { id: f; x: undefined; opacity: undefined }
    Item { id: g; property bool on: true; property int i: on ? 6 : undefined; property real r: 2.5; property string s: "s"; property var v: 1 }
    Item { id: h; property int pi: 8 }
    Item { id: k; opacity: 0.25; anchors.leftMargin: 4 }
    property int n: 5
    function t(f) { try { f(); return "" } catch (x) { return String(x) } }
    property string result
    Component.onCompleted: {
        e.on = false
        g.on = false
        g.v = undefined
        d.width = undefined
        k.anchors.leftMargin = undefined
        result = [a.width, b.width, c.width, d.width, e.width, f.x, f.opacity, g.i, g.r, g.s, g.v, h.pi,
            t(() => { h.pi = undefined }), n, t(() => { n = undefined }), t(() => { k.opacity = undefined }), k.opacity,
            k.anchors.leftMargin, a.height].map(String).join(" ")
    }
}
