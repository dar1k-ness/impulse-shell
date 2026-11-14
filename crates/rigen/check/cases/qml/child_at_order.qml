// Item.childAt(x, y): the last child in the order of `children` whose `visible` is true and
// which contains the point wins; `z`, `opacity`, `enabled` and the parent's `clip` and size
// do not count, a child of zero size contains nothing, a grandchild is never returned.
// qt: order a,b,c,c,null;z low;visible shown;opacity clear,disabled,under;size sized,sized,sized,sized;clip outside,before,null;noSize inNoSize;deep outer,null,outer;hiddenParent null;edges box,null,box,null,null,null;nothing null true object;same true
import QtQuick
Item {
    id: root
    width: 100; height: 100
    property string result: ""
    function n(i) { return i === null ? "null" : i === undefined ? "undefined" : i.objectName }
    Item {
        id: order; width: 100; height: 100
        Item { objectName: "a"; width: 50; height: 50 }
        Item { objectName: "b"; x: 25; y: 25; width: 50; height: 50 }
        Item { objectName: "c"; x: 40; y: 40; width: 50; height: 50 }
    }
    Item {
        id: zs; width: 100; height: 100
        Item { objectName: "top"; z: 5; width: 50; height: 50 }
        Item { objectName: "mid"; width: 50; height: 50 }
        Item { objectName: "low"; z: -1; width: 50; height: 50 }
    }
    Item {
        id: hidden; width: 100; height: 100
        Item { objectName: "shown"; width: 50; height: 50 }
        Item { objectName: "invisible"; visible: false; width: 50; height: 50 }
    }
    Item {
        id: faint; width: 100; height: 100
        Item { objectName: "under"; width: 50; height: 50 }
        Item { objectName: "clear"; opacity: 0; width: 30; height: 30 }
        Item { objectName: "disabled"; enabled: false; x: 30; width: 20; height: 20 }
    }
    Item {
        id: sizes; width: 100; height: 100
        Item { objectName: "sized"; width: 60; height: 60 }
        Item { objectName: "empty"; x: 10; y: 10 }
        Item { objectName: "flat"; x: 20; y: 20; width: 10 }
        Item { objectName: "negative"; x: 40; y: 40; width: -10; height: -10 }
    }
    Item {
        id: clipped; width: 20; height: 20; clip: true
        Item { objectName: "outside"; x: 50; y: 50; width: 20; height: 20 }
        Item { objectName: "before"; x: -30; y: -30; width: 20; height: 20 }
    }
    Item {
        id: noSize
        Item { objectName: "inNoSize"; width: 20; height: 20 }
    }
    Item {
        id: deep; width: 100; height: 100
        Item {
            objectName: "outer"; width: 40; height: 40
            Item { objectName: "inner"; width: 20; height: 20 }
            Item { objectName: "overflow"; x: 60; width: 20; height: 20 }
        }
    }
    Item {
        id: off; width: 100; height: 100; visible: false
        Item { objectName: "inHidden"; width: 50; height: 50 }
    }
    Item {
        id: edges; width: 100; height: 100
        Item { objectName: "box"; x: 10; y: 10; width: 20; height: 20 }
    }
    Component.onCompleted: {
        var out = []
        out.push("order " + [order.childAt(10, 10), order.childAt(30, 30), order.childAt(45, 45), order.childAt(80, 80), order.childAt(95, 95)].map(n).join(","))
        out.push("z " + n(zs.childAt(10, 10)))
        out.push("visible " + n(hidden.childAt(10, 10)))
        out.push("opacity " + [faint.childAt(10, 10), faint.childAt(35, 10), faint.childAt(45, 45)].map(n).join(","))
        out.push("size " + [sizes.childAt(10, 10), sizes.childAt(25, 20), sizes.childAt(35, 35), sizes.childAt(40, 40)].map(n).join(","))
        out.push("clip " + [clipped.childAt(55, 55), clipped.childAt(-20, -20), clipped.childAt(10, 10)].map(n).join(","))
        out.push("noSize " + n(noSize.childAt(5, 5)))
        out.push("deep " + [deep.childAt(10, 10), deep.childAt(65, 5), deep.childAt(30, 30)].map(n).join(","))
        out.push("hiddenParent " + n(off.childAt(10, 10)))
        out.push("edges " + [edges.childAt(10, 10), edges.childAt(9.999, 10), edges.childAt(29.999, 29.999), edges.childAt(30, 20), edges.childAt(20, 30), edges.childAt(10, 9.999)].map(n).join(","))
        var nothing = edges.childAt(50, 50)
        out.push("nothing " + nothing + " " + (nothing === null) + " " + typeof nothing)
        out.push("same " + (order.childAt(45, 45) === order.children[2]))
        result = out.join(";")
    }
}
