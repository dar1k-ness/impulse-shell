// Item.childAt and Item.contains with `transform` lists, as in Qt 6.12: the point goes into
// the child through the same matrix mapToItem uses. `contains` takes the item's own size, or
// its `containmentMask` item for the point moved back by the mask's position.
// qt: t3 t1 t1 null t2 t2 null null null null null null t4 t4 t4 null mk null rr rr ma | hk null | true false true false false true true true TypeError: Passing incompatible arguments to C++ functions from JavaScript is not allowed. Error: Insufficient arguments true true TypeError: Passing incompatible arguments to C++ functions from JavaScript is not allowed. true
import QtQuick
Item {
    id: root
    width: 300; height: 300
    property string result: ""
    function n(o) { return o === null ? "null" : o === undefined ? "undefined" : (o.objectName || "?") }
    function at(o, x, y) { var r; try { r = n(o.childAt(x, y)) } catch (e) { r = "" + e } return r }
    // transform list on the child, and on the item asked
    Item { id: t1; objectName: "t1"; x: 10; y: 10; width: 20; height: 20; transform: Translate { x: 100 } }
    Item { id: t2; objectName: "t2"; x: 10; y: 50; width: 20; height: 20; scale: 2; transform: [ Scale { xScale: 2 }, Rotation { angle: 90 } ] }
    Item { id: t3; objectName: "t3"; x: 10; y: 150; width: 20; height: 20; transform: Scale { xScale: 0 } }
    Item { id: t4; objectName: "t4"; x: 200; y: 10; width: 40; height: 40; transform: Rotation { axis { x: 0; y: 1; z: 0 } angle: 60; origin.x: 20 } }
    Item { id: host; objectName: "host"; x: 100; y: 200; width: 50; height: 50; scale: 2; rotation: 45
        Item { id: hk; objectName: "hk"; x: 10; y: 10; width: 10; height: 10 } }
    // containment mask
    Item { id: mk; objectName: "mk"; x: 200; y: 100; width: 40; height: 40; containmentMask: Item { x: 0; y: 0; width: 10; height: 10 } }
    Rectangle { id: rr; objectName: "rr"; x: 250; y: 250; width: 40; height: 40; radius: 20 }
    MouseArea { id: ma; objectName: "ma"; x: 250; y: 200; width: 20; height: 20; enabled: false }
    Component.onCompleted: {
        var pts = [[15,15],[115,15],[129,29],[130,30],[9,60],[-10,60],[-30,50],[-30,129],[-31,60],[10,155],[15,155],[205,15],[215,30],[225,30],[229,30],[231,30],
                   [205,105],[215,115],[251,251],[270,270],[255,205]]
        for (var i = 0; i < pts.length; i++) result += at(root, pts[i][0], pts[i][1]) + " "
        result += "| " + at(host, 15, 15) + " " + at(host, 25, 25)
        result += " | " + mk.contains(Qt.point(5, 5)) + " " + mk.contains(Qt.point(15, 15)) + " " + t1.contains(Qt.point(0, 0)) + " " + t1.contains(Qt.point(20, 0)) + " " + t1.contains(Qt.point(-0.1, 0)) + " " + t1.contains(Qt.point(19.999, 19.999)) + " " + rr.contains(Qt.point(1, 1)) + " " + t3.contains(Qt.point(1, 1))
        var e = ""; try { e = t1.contains(1, 1) } catch (x) { e = x } result += " " + e
        try { e = t1.contains() } catch (x) { e = x } result += " " + e
        try { e = t1.contains(Qt.rect(1, 1, 1, 1)) } catch (x) { e = x } result += " " + e
        try { e = t1.contains({ x: 1, y: 1 }) } catch (x) { e = x } result += " " + e
        try { e = t1.contains(null) } catch (x) { e = x } result += " " + e
        try { e = t1.contains(Qt.vector2d(1, 1)) } catch (x) { e = x } result += " " + e
    }
}
