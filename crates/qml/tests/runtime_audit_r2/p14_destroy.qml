import QtQuick
Item {
    id: r
    Item { id: kid; }
    QtObject { id: obj }
    Component { id: c; Item { Component.onDestruction: console.log("dyn destroyed") } }
    Loader { id: ld; sourceComponent: c }
    Repeater { id: rep; model: 1; Item { } }
    function attempt(what, o) {
        try { o.destroy(); console.log(what, "no throw") } catch (e) { console.log(what, "threw:", e.message) }
    }
    Component.onCompleted: {
        var d = c.createObject(r)
        var f = c.createObject(null)
        var g = c.createObject(r)
        var q = Qt.createQmlObject("import QtQuick; Item { Item { objectName: 'in' } }", r)
        attempt("root", r)
        attempt("static child", kid)
        attempt("static object", obj)
        attempt("loader item", ld.item)
        attempt("delegate", rep.itemAt(0))
        attempt("createObject", d)
        attempt("createObject twice", d)
        attempt("createObject null parent", f)
        attempt("createQmlObject child", q.children[0])
        attempt("createQmlObject", q)
        try { g.destroy(10); console.log("delay ok") } catch (e) { console.log("delay threw", e.message) }
        attempt("component", c)
    }
}
