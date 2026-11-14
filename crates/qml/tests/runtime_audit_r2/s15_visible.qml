import QtQuick
Item {
    id: r
    Item { id: a
        Item { id: b; onVisibleChanged: console.log("b.visibleChanged", visible)
            Item { id: c; visible: false; onVisibleChanged: console.log("c.visibleChanged", visible) }
        }
        Item { id: d }
    }
    property Item orphan: Item { id: o; Item { id: oc } }
    Component.onCompleted: {
        console.log("init r", r.visible, "a", a.visible, "orphan", o.visible, "oc", oc.visible)
        console.log("visibleChildren", a.visibleChildren.length)
        a.visible = false; console.log("b", b.visible, "c", c.visible, "vc", a.visibleChildren.length, r.visibleChildren.length)
        b.visible = true; console.log("b after own true", b.visible)
        a.visible = true; console.log("b", b.visible, "c", c.visible)
        d.parent = null; console.log("d unparented", d.visible)
        d.visible = false; d.parent = a; d.visible = true; console.log("d", d.visible)
    }
}
