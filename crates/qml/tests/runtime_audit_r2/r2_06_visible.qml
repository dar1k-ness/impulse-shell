import QtQuick
Item {
    id: r
    Item { id: a
        Item { id: b; onVisibleChanged: console.log("b.visibleChanged", visible)
            Item { id: c; visible: false; onVisibleChanged: console.log("c.visibleChanged", visible) }
        }
        Item { id: d; onVisibleChanged: console.log("d.visibleChanged", visible) }
    }
    Item { id: hid; visible: false }
    Component { id: comp; Item { onVisibleChanged: console.log("dyn.visibleChanged", visible) } }
    property Item orphan: Item { id: o; Item { id: oc } }
    Component.onCompleted: {
        console.log("init r", r.visible, "a", a.visible, "orphan", o.visible, "oc", oc.visible)
        a.visible = false; console.log("b", b.visible, "c", c.visible)
        b.visible = true; console.log("b after own true", b.visible)
        a.visible = true; console.log("b", b.visible, "c", c.visible)
        d.parent = null; console.log("d unparented", d.visible)
        d.visible = false; d.parent = a; d.visible = true; console.log("d", d.visible)
        d.parent = null; d.parent = hid; console.log("d under hidden", d.visible)
        d.parent = a; console.log("d back", d.visible)
        d.visible = false; d.parent = null; console.log("d hidden unparented", d.visible)
        d.visible = true; console.log("d shown unparented", d.visible)
        d.parent = a; console.log("d back again", d.visible)
        var n = comp.createObject(null); console.log("dyn without a parent", n.visible)
        n.visible = false; n.visible = true; console.log("dyn toggled", n.visible)
        n.parent = a; console.log("dyn parented", n.visible)
        n.parent = null; console.log("dyn unparented", n.visible)
    }
}
