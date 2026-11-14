// Item.contains(point) as in Qt 6.12: any object is a point of its `x` and `y` (0 for one it
// does not have), anything else is a TypeError; an item set as `containmentMask` answers
// for the point moved back by its position, an object without contains(QPointF) is ignored.
// qt: false true false true true false true true true true true TypeError: Pas TypeError: Pas TypeError: Pas TypeError: Pas TypeError: Pas true TypeError: Pas false false false true false false true false true false false false true false true false true true true true true true false false true TypeError: Pas | true false true false false false true false | true true false true true true null | true false false true function
import QtQuick
Item {
    id: root
    width: 300; height: 300
    property string result: ""
    Item { id: t1; x: 10; y: 10; width: 20; height: 20 }
    Item { id: mk; objectName: "mk"; x: 200; y: 100; width: 40; height: 40; containmentMask: Item { id: m1; x: 5; y: 5; width: 10; height: 10 } }
    Item { id: mq; x: 100; y: 100; width: 40; height: 40; containmentMask: QtObject { function contains(p) { return p.x > 30 } } }
    Item { id: far; x: 50; y: 50 }
    QtObject { id: qo; property real x: 50; property real y: 50 }
    Item { id: m2; width: 100; height: 5; scale: 2 }
    function c(o, v) { var e; try { e = o.contains(v) } catch (x) { e = ("" + x).substring(0, 14) } return e + " " }
    Component.onCompleted: {
        var vals = [Qt.rect(50, 50, 1, 1), Qt.rect(1, 1, 50, 50), { x: 50, y: 50 }, { x: 1, y: 1 }, {}, Qt.vector2d(50, 50), Qt.vector2d(1, 1), Qt.size(50, 50), Qt.size(1, 1),
                    [1, 1], [50, 50], "1,1", "50,50", 5, 50, undefined, t1, true, Qt.vector3d(50, 1, 1), Qt.point(NaN, 1), Qt.point(Infinity, 1), Qt.point(-0, -0)]
        for (var i = 0; i < vals.length; i++) result += c(t1, vals[i])
        var more = [far, qo, root, { x: "50" }, { x: "5" }, { x: 50 }, { y: 50 }, { x: NaN }, { x: 1, y: 1, z: 9 }, Qt.vector4d(50, 1, 1, 1), Qt.vector4d(1, 1, 1, 1), Qt.quaternion(1, 50, 1, 1),
                    function() {}, new Date(), Qt.rgba(1, 1, 1, 1), Qt.font({ pixelSize: 5 }), [50], Qt.point(1, 1), Qt.rect(-1, 1, 5, 5), { x: { valueOf: function() { return 50 } } }, Qt.matrix4x4(), Symbol()]
        for (i = 0; i < more.length; i++) result += c(t1, more[i])
        result += "| " + c(mk, Qt.point(5, 5)) + c(mk, Qt.point(4, 4)) + c(mk, Qt.point(14, 14)) + c(mk, Qt.point(15, 15)) + c(mk, Qt.point(0, 0)) + c(mk, Qt.point(39, 39)) + c(m1, Qt.point(1, 1)) + c(m1, Qt.point(12, 12))
        result += "| " + c(mq, Qt.point(35, 1)) + c(mq, Qt.point(5, 5)) + c(mq, Qt.point(50, 50)) + (root.childAt(101, 101) === mq) + " " + (root.childAt(135, 101) === mq) + " " + (mk.containmentMask === m1) + " " + t1.containmentMask
        mk.containmentMask = m2; result += " | " + c(mk, Qt.point(90, 3)) + c(mk, Qt.point(3, 30)) + c(mk, Qt.point(3, 7))
        mk.containmentMask = null; result += c(mk, Qt.point(30, 30))
        result += typeof t1.contains
    }
}
