// A script writes `undefined` to a property that takes none (no RESET, not a `var`), with a
// binding (`Bound`) and with none (`Plain`): the error, the value left, the times the change
// signal came, then the same after what the binding followed has changed. Qt takes the binding
// off before it refuses the value (QV4::QObjectWrapper::setProperty), so the value stays.
// `width` and a Rectangle's `color` have a RESET, `font.pixelSize` takes it in silence, a
// `var` keeps `undefined`; the last row is written nothing and follows its binding.
// qt: bound Item.opacity: 0.25 -> Error: Cannot assign [undefined] to double -> 0.25 signals 0 -> 0.25 signals 0 | Item.clip: true -> Error: Cannot assign [undefined] to bool -> true signals 0 -> true signals 0 | Item.width: 10 -> ok -> 33 signals 1 -> 33 signals 1 | Grid.columns: 2 -> Error: Cannot assign [undefined] to int -> 2 signals 0 -> 2 signals 0 | Text.text: t1 -> Error: Cannot assign [undefined] to QString -> t1 signals 0 -> t1 signals 0 | Rectangle.color: #ff0000 -> ok -> #ffffff signals 1 -> #ffffff signals 1 | Item.parent: <one> -> Error: Cannot assign [undefined] to QQuickItem* -> <one> signals 0 -> <one> signals 0 | anchors.margins: 3 -> Error: Cannot assign [undefined] to double -> 3 signals 0 -> 3 signals 0 | anchors.horizontalCenterOffset: 4 -> Error: Cannot assign [undefined] to double -> 4 signals 0 -> 4 signals 0 | border.width: 2 -> Error: Cannot assign [undefined] to double -> 2 signals 0 -> 2 signals 0 | font.pixelSize: 11 -> ok -> 11 signals 0 -> 11 signals 0 | real: 0.25 -> Error: Cannot assign [undefined] to double -> 0.25 signals 0 -> 0.25 signals 0 | int: 2 -> Error: Cannot assign [undefined] to int -> 2 signals 0 -> 2 signals 0 | bool: true -> Error: Cannot assign [undefined] to bool -> true signals 0 -> true signals 0 | string: t1 -> Error: Cannot assign [undefined] to QString -> t1 signals 0 -> t1 signals 0 | color: #ff0000 -> Error: Cannot assign [undefined] to QColor -> #ff0000 signals 0 -> #ff0000 signals 0 | url: a.txt -> Error: Cannot assign [undefined] to QUrl -> a.txt signals 0 -> a.txt signals 0 | Item: <one> -> Error: Cannot assign [undefined] to QQuickItem* -> <one> signals 0 -> <one> signals 0 | Rectangle: <re> -> Error: Cannot assign [undefined] to QQuickRectangle* -> <re> signals 0 -> <re> signals 0 | alias of Item.opacity: 0.25 -> Error: Cannot assign [undefined] to double -> 0.25 signals 0 -> 0.25 signals 0 | var: 101 -> ok -> undefined signals 1 -> undefined signals 1 | nothing written: 1 -> ok -> 1 signals 0 -> 7 signals 1 | plain Item.opacity: 0.25 -> Error: Cannot assign [undefined] to double -> 0.25 signals 0 -> 0.25 signals 0 | Item.clip: true -> Error: Cannot assign [undefined] to bool -> true signals 0 -> true signals 0 | Item.width: 10 -> ok -> 33 signals 1 -> 33 signals 1 | Grid.columns: 2 -> Error: Cannot assign [undefined] to int -> 2 signals 0 -> 2 signals 0 | Text.text: t1 -> Error: Cannot assign [undefined] to QString -> t1 signals 0 -> t1 signals 0 | Rectangle.color: #ff0000 -> ok -> #ffffff signals 1 -> #ffffff signals 1 | Item.parent: <one> -> Error: Cannot assign [undefined] to QQuickItem* -> <one> signals 0 -> <one> signals 0 | anchors.margins: 3 -> Error: Cannot assign [undefined] to double -> 3 signals 0 -> 3 signals 0 | anchors.horizontalCenterOffset: 4 -> Error: Cannot assign [undefined] to double -> 4 signals 0 -> 4 signals 0 | border.width: 2 -> Error: Cannot assign [undefined] to double -> 2 signals 0 -> 2 signals 0 | font.pixelSize: 11 -> ok -> 11 signals 0 -> 11 signals 0 | real: 0.25 -> Error: Cannot assign [undefined] to double -> 0.25 signals 0 -> 0.25 signals 0 | int: 2 -> Error: Cannot assign [undefined] to int -> 2 signals 0 -> 2 signals 0 | bool: true -> Error: Cannot assign [undefined] to bool -> true signals 0 -> true signals 0 | string: t1 -> Error: Cannot assign [undefined] to QString -> t1 signals 0 -> t1 signals 0 | color: #ff0000 -> Error: Cannot assign [undefined] to QColor -> #ff0000 signals 0 -> #ff0000 signals 0 | url: a.txt -> Error: Cannot assign [undefined] to QUrl -> a.txt signals 0 -> a.txt signals 0 | Item: <one> -> Error: Cannot assign [undefined] to QQuickItem* -> <one> signals 0 -> <one> signals 0 | Rectangle: <re> -> Error: Cannot assign [undefined] to QQuickRectangle* -> <re> signals 0 -> <re> signals 0 | alias of Item.opacity: 0.25 -> Error: Cannot assign [undefined] to double -> 0.25 signals 0 -> 0.25 signals 0 | var: 101 -> ok -> undefined signals 1 -> undefined signals 1 | nothing written: 1 -> ok -> 1 signals 0 -> 1 signals 0
import QtQuick
Item {
    id: root
    function show(v) {
        if (v === null || v === undefined) return String(v);
        if (typeof v === "object" && "objectName" in v) return "<" + v.objectName + ">";
        return String(v);
    }
    // Each property is bound to `k`; `rows()` names them.
    component Bound: Item {
        id: o
        property real k: 1
        property alias ao: it2.opacity
        // What tells that a field of a group has changed.
        property real wm: an.anchors.margins
        property real wh: an.anchors.horizontalCenterOffset
        property real wb: re.border.width
        Item { id: one; objectName: "one" }
        Item { id: two; objectName: "two" }
        Item { id: it; opacity: o.k / 4; clip: o.k < 5; width: o.k * 10; implicitWidth: 33 }
        Item { id: it2; opacity: o.k / 4 }
        Item { id: un; z: o.k }
        Grid { id: gr; columns: o.k + 1 }
        Text { id: tx; text: "t" + o.k; font.pixelSize: o.k + 10 }
        Rectangle { id: re; objectName: "re"; color: o.k < 5 ? "#ff0000" : "#0000ff"; border.width: o.k + 1 }
        Item { id: pa; objectName: "pa"; parent: o.k < 5 ? one : two }
        Item { id: an; anchors.margins: o.k + 2; anchors.horizontalCenterOffset: o.k + 3 }
        QtObject {
            id: h
            property real r: o.k / 4
            property int i: o.k + 1
            property bool b: o.k < 5
            property string s: "t" + o.k
            property color c: o.k < 5 ? "#ff0000" : "#0000ff"
            property url u: o.k < 5 ? "a.txt" : "b.txt"
            property Item ob: o.k < 5 ? one : two
            property Rectangle rc: o.k < 5 ? re : null
            property var v: o.k + 100
        }
        function rows() {
            return [
                { n: "Item.opacity", get: () => it.opacity, put: v => { it.opacity = v }, on: f => it.opacityChanged.connect(f) },
                { n: "Item.clip", get: () => it.clip, put: v => { it.clip = v }, on: f => it.clipChanged.connect(f) },
                { n: "Item.width", get: () => it.width, put: v => { it.width = v }, on: f => it.widthChanged.connect(f) },
                { n: "Grid.columns", get: () => gr.columns, put: v => { gr.columns = v }, on: f => gr.columnsChanged.connect(f) },
                { n: "Text.text", get: () => tx.text, put: v => { tx.text = v }, on: f => tx.textChanged.connect(f) },
                { n: "Rectangle.color", get: () => re.color, put: v => { re.color = v }, on: f => re.colorChanged.connect(f) },
                { n: "Item.parent", get: () => pa.parent, put: v => { pa.parent = v }, on: f => pa.parentChanged.connect(f) },
                { n: "anchors.margins", get: () => an.anchors.margins, put: v => { an.anchors.margins = v }, on: f => o.wmChanged.connect(f) },
                { n: "anchors.horizontalCenterOffset", get: () => an.anchors.horizontalCenterOffset, put: v => { an.anchors.horizontalCenterOffset = v }, on: f => o.whChanged.connect(f) },
                { n: "border.width", get: () => re.border.width, put: v => { re.border.width = v }, on: f => o.wbChanged.connect(f) },
                { n: "font.pixelSize", get: () => tx.font.pixelSize, put: v => { tx.font.pixelSize = v }, on: f => tx.fontChanged.connect(f) },
                { n: "real", get: () => h.r, put: v => { h.r = v }, on: f => h.rChanged.connect(f) },
                { n: "int", get: () => h.i, put: v => { h.i = v }, on: f => h.iChanged.connect(f) },
                { n: "bool", get: () => h.b, put: v => { h.b = v }, on: f => h.bChanged.connect(f) },
                { n: "string", get: () => h.s, put: v => { h.s = v }, on: f => h.sChanged.connect(f) },
                { n: "color", get: () => h.c, put: v => { h.c = v }, on: f => h.cChanged.connect(f) },
                { n: "url", get: () => h.u, put: v => { h.u = v }, on: f => h.uChanged.connect(f) },
                { n: "Item", get: () => h.ob, put: v => { h.ob = v }, on: f => h.obChanged.connect(f) },
                { n: "Rectangle", get: () => h.rc, put: v => { h.rc = v }, on: f => h.rcChanged.connect(f) },
                { n: "alias of Item.opacity", get: () => o.ao, put: v => { o.ao = v }, on: f => o.aoChanged.connect(f) },
                { n: "var", get: () => h.v, put: v => { h.v = v }, on: f => h.vChanged.connect(f) },
                // Not written: with a binding it follows `k`.
                { n: "nothing written", get: () => un.z, put: v => {}, on: f => un.zChanged.connect(f) }
            ]
        }
    }
    // The same properties with the value each has there for `k: 1`, and no binding.
    component Plain: Item {
        id: o
        property real k: 1
        property alias ao: it2.opacity
        // What tells that a field of a group has changed.
        property real wm: an.anchors.margins
        property real wh: an.anchors.horizontalCenterOffset
        property real wb: re.border.width
        Item { id: one; objectName: "one" }
        Item { id: two; objectName: "two" }
        Item { id: it; opacity: 0.25; clip: true; width: 10; implicitWidth: 33 }
        Item { id: it2; opacity: 0.25 }
        Item { id: un; z: 1 }
        Grid { id: gr; columns: 2 }
        Text { id: tx; text: "t1"; font.pixelSize: 11 }
        Rectangle { id: re; objectName: "re"; color: "#ff0000"; border.width: 2 }
        Item { id: pa; objectName: "pa"; parent: one }
        Item { id: an; anchors.margins: 3; anchors.horizontalCenterOffset: 4 }
        QtObject {
            id: h
            property real r: 0.25
            property int i: 2
            property bool b: true
            property string s: "t1"
            property color c: "#ff0000"
            property url u: "a.txt"
            property Item ob: one
            property Rectangle rc: re
            property var v: 101
        }
        function rows() {
            return [
                { n: "Item.opacity", get: () => it.opacity, put: v => { it.opacity = v }, on: f => it.opacityChanged.connect(f) },
                { n: "Item.clip", get: () => it.clip, put: v => { it.clip = v }, on: f => it.clipChanged.connect(f) },
                { n: "Item.width", get: () => it.width, put: v => { it.width = v }, on: f => it.widthChanged.connect(f) },
                { n: "Grid.columns", get: () => gr.columns, put: v => { gr.columns = v }, on: f => gr.columnsChanged.connect(f) },
                { n: "Text.text", get: () => tx.text, put: v => { tx.text = v }, on: f => tx.textChanged.connect(f) },
                { n: "Rectangle.color", get: () => re.color, put: v => { re.color = v }, on: f => re.colorChanged.connect(f) },
                { n: "Item.parent", get: () => pa.parent, put: v => { pa.parent = v }, on: f => pa.parentChanged.connect(f) },
                { n: "anchors.margins", get: () => an.anchors.margins, put: v => { an.anchors.margins = v }, on: f => o.wmChanged.connect(f) },
                { n: "anchors.horizontalCenterOffset", get: () => an.anchors.horizontalCenterOffset, put: v => { an.anchors.horizontalCenterOffset = v }, on: f => o.whChanged.connect(f) },
                { n: "border.width", get: () => re.border.width, put: v => { re.border.width = v }, on: f => o.wbChanged.connect(f) },
                { n: "font.pixelSize", get: () => tx.font.pixelSize, put: v => { tx.font.pixelSize = v }, on: f => tx.fontChanged.connect(f) },
                { n: "real", get: () => h.r, put: v => { h.r = v }, on: f => h.rChanged.connect(f) },
                { n: "int", get: () => h.i, put: v => { h.i = v }, on: f => h.iChanged.connect(f) },
                { n: "bool", get: () => h.b, put: v => { h.b = v }, on: f => h.bChanged.connect(f) },
                { n: "string", get: () => h.s, put: v => { h.s = v }, on: f => h.sChanged.connect(f) },
                { n: "color", get: () => h.c, put: v => { h.c = v }, on: f => h.cChanged.connect(f) },
                { n: "url", get: () => h.u, put: v => { h.u = v }, on: f => h.uChanged.connect(f) },
                { n: "Item", get: () => h.ob, put: v => { h.ob = v }, on: f => h.obChanged.connect(f) },
                { n: "Rectangle", get: () => h.rc, put: v => { h.rc = v }, on: f => h.rcChanged.connect(f) },
                { n: "alias of Item.opacity", get: () => o.ao, put: v => { o.ao = v }, on: f => o.aoChanged.connect(f) },
                { n: "var", get: () => h.v, put: v => { h.v = v }, on: f => h.vChanged.connect(f) },
                // Not written: with a binding it follows `k`.
                { n: "nothing written", get: () => un.z, put: v => {}, on: f => un.zChanged.connect(f) }
            ]
        }
    }
    Bound { id: bound }
    Plain { id: plain }
    function run(o) {
        var rows = o.rows(), n = [], out = [];
        rows.forEach((r, i) => { n.push(0); r.on(() => { n[i]++ }) });
        rows.forEach((r, i) => {
            var s = show(r.get()) + " -> ";
            try { r.put(undefined); s += "ok" } catch (e) { s += e }
            out.push(s + " -> " + show(r.get()) + " signals " + n[i]);
        });
        o.k = 7;
        return rows.map((r, i) => r.n + ": " + out[i] + " -> " + show(r.get()) + " signals " + n[i]);
    }
    property string result
    Component.onCompleted: result = "bound " + run(bound).join(" | ") + " | plain " + run(plain).join(" | ")
}
