// Item.childAt where a transform has no inverse (a zero scale), as in Qt 6.12: mapToItem
// into such a child is the scene point, so the child is found where the scene point lies
// within its size.
// qt: null ok flat flat flat null | null null null null null null sq | d1 d1 null deep deep null
import QtQuick
Item {
    id: root
    width: 300; height: 300
    property string result: ""
    function n(o) { return o === null ? "null" : (o.objectName || "?") }
    Item { id: far; objectName: "far"; x: 100; y: 100; width: 100; height: 100
        Item { id: flat; objectName: "flat"; x: 50; y: 50; width: 30; height: 30; transform: Scale { xScale: 0 } }
        Item { id: ok; objectName: "ok"; x: 0; y: 0; width: 10; height: 10 }
    }
    Item { id: sq; objectName: "sq"; x: 200; y: 0; width: 50; height: 50; transform: Scale { yScale: 0 }
        Item { id: in1; objectName: "in1"; x: 10; y: 10; width: 20; height: 20 }
    }
    Item { id: deep; objectName: "deep"; x: 10; y: 200; width: 50; height: 50; rotation: 90; transformOrigin: Item.TopLeft
        Item { id: d1; objectName: "d1"; x: 10; y: 0; width: 30; height: 10; scale: 2; transformOrigin: Item.TopLeft }
    }
    Component.onCompleted: {
        // A child without an inverse: mapToItem(child, p) is the scene point (identity).
        result += n(far.childAt(60, 60)) + " " + n(far.childAt(5, 5)) + " " + n(far.childAt(-80, -80)) + " " + n(far.childAt(-100, -100)) + " " + n(far.childAt(-71, -71)) + " " + n(far.childAt(-70, -70))
        // The item asked has no inverse itself: its children are still found.
        result += " | " + n(sq.childAt(15, 15)) + " " + n(sq.childAt(215, 15)) + " " + n(sq.childAt(15, 0)) + " " + n(sq.childAt(5, 5)) + " " + n(root.childAt(210, 0)) + " " + n(root.childAt(210, 10)) + " " + n(root.childAt(10, 10))
        result += " | " + n(deep.childAt(15, 5)) + " " + n(deep.childAt(69, 19)) + " " + n(deep.childAt(70, 5)) + " " + n(root.childAt(5, 205)) + " " + n(root.childAt(-10, 215)) + " " + n(root.childAt(11, 215))
    }
}
