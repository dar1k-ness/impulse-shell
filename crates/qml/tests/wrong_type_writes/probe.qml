import QtQuick
Item {
    id: root
    Item { id: other }
    Rectangle { id: rc }
    Text { id: tx; height: 17 }
    QtObject { id: plain }
    Component { id: comp; Item {} }
    /*S*/
    /*B*/
    property string path: "/*PATH*/"
    property var good: () => /*GOOD*/
    property var values: [
/*VALUES*/
    ]
    function show(v) {
        if (v === other) return "item"
        if (v === plain) return "object"
        if (v === comp) return "component"
        if (v === rc) return "rectangle"
        if (v === tx) return "text"
        if (v === null) return "null"
        var t = typeof v
        if (t === "object" && v instanceof Date) return "date:" + v.getTime()
        if (t === "object" && typeof v.length === "number") {
            var a = []
            for (var i = 0; i < v.length; i++) a.push(show(v[i]))
            return "list[" + a.join(",") + "]"
        }
        if (t === "object" && v.family !== undefined) return "font:" + v.family
        if (t === "object" && v.hsvHue !== undefined) return "color:" + v + ":" + v.a + ":" + v.valid
        if (t === "object" && String(v) === "QQuickAnchorLine()")
            return "line:" + (v == other.right ? "right" : v == other.left ? "left" : v == root.left ? "none" : "other")
        if (t === "symbol") return "symbol"
        return t + ":" + v
    }
    function get(o) { path.split(".").forEach(k => o = o[k]); return o }
    function set(o, v) {
        var ks = path.split(".")
        for (var i = 0; i < ks.length - 1; i++) o = o[ks[i]]
        o[ks[ks.length - 1]] = v
    }
    // Value `i` written by a script over the good value.
    function script(i) {
        try { set(s, good()) } catch (e) {}
        try { set(s, values[i]()); return show(get(s)) } catch (e) { return "!" + e }
    }
    // The binding `path: v` gives value `i` after the good value.
    function bind(i) {
        b.v = good()
        console.log("CASE")
        try { b.v = values[i](); return show(get(b)) } catch (e) { return "!" + e }
    }
}
