import QtQuick
Item {
    id: r
    Item { id: a; objectName: "a"
        onVisibleChildrenChanged: console.log("a.vcc", visibleChildren.length)
        onVisibleChanged: console.log("a.visible", visible)
        Item { id: b; objectName: "b"; onVisibleChildrenChanged: console.log("b.vcc", visibleChildren.length)
            Item { id: c; visible: false }
            Item { id: c2 }
        }
        Item { id: d; objectName: "d" }
        QtObject { id: q }
    }
    Item { id: e; onVisibleChildrenChanged: console.log("e.vcc", visibleChildren.length) }
    property Item pv: Item { id: o }
    function names(l) { var s = []; for (var i = 0; i < l.length; i++) s.push(l[i].objectName); return s.join(",") }
    Component.onCompleted: {
        console.log("init", names(a.visibleChildren), b.visibleChildren.length, r.visibleChildren.length, typeof a.visibleChildren, a.visibleChildren === a.visibleChildren)
        console.log("1"); d.visible = false
        console.log("2"); d.visible = true
        console.log("3"); c.visible = true
        console.log("4"); a.visible = false
        console.log("5", names(a.visibleChildren)); d.visible = false
        console.log("6"); a.visible = true
        console.log("7", names(a.visibleChildren)); d.parent = e
        console.log("8"); d.visible = true
        console.log("9"); d.parent = a
        console.log("10", names(a.visibleChildren)); b.parent = null
        console.log("11", names(a.visibleChildren)); b.parent = a
        console.log("12", names(a.visibleChildren))
        try { a.visibleChildren = []; console.log("w", a.visibleChildren.length) } catch (x) { console.log("err", x.message) }
        var k = Qt.createQmlObject("import QtQuick; Item { objectName: 'k' }", a)
        console.log("13", names(a.visibleChildren)); k.destroy()
    }
    Timer { interval: 10; running: true; onTriggered: console.log("end", r.names(a.visibleChildren)) }
}
