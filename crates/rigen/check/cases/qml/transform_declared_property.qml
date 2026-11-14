// A property declared in QML under the name of a C++ property of a transform or of an Item
// hides it (QQmlPropertyCache::append) here too: script reads, writes and is notified of the
// declared one, and the transform is made of the C++ property, which stays at its default
// since nothing can set it. `origin` and `axis` of a Rotation and a Scale (also declared as
// a vector3d, and in an inline component), `angle`, `xScale`, `yScale`, `x`, `y` of a
// Translate, `xFactor`, `matrix`, and `rotation` and `transformOrigin` of an Item. The last
// entry of the first half has `origin` undeclared: its fields move the rotation.
// qt: mine 7 mine! -10,20 | QVector3D(10, 10, 0) QVector3D(1, 0, 0) 10 -10,20 | [1,2] 40,10 | QVector3D(10, 10, 0) 40,20 | c -10,20 | QVector3D(10, 10, 0) 10,20 || w 9 w! -10,20 | QVector3D(5, 20, 0) -10,20 | QVector3D(10, 3, 0) 40,20 | oa ## 90 20,10 | 3 3 20,10 | 7 q 20,10 | 2 20,10 | 90 20,10 | 1 20,10 | m 20,10 | 0 30,10
import QtQuick
Item {
    id: root
    property string result: ""
    property string log: ""
    width: 100; height: 100
    component Turn: Rotation { property string origin: "c"; angle: 90 }
    Item {
        id: a1; width: 20; height: 20
        transform: Rotation {
            id: r1
            property string origin: "mine"
            property int axis: 7
            property string seen: r1.origin + "!"
            onOriginChanged: root.log += "o"
            onAxisChanged: root.log += "a"
            angle: 90
        }
    }
    Item {
        id: b1; width: 20; height: 20
        transform: Rotation {
            id: r2
            property vector3d origin: Qt.vector3d(10, 10, 0)
            property vector3d axis: Qt.vector3d(1, 0, 0)
            angle: 90
        }
    }
    Item {
        id: c1; width: 20; height: 20
        transform: Scale {
            id: s1
            property var origin: [1, 2]
            xScale: 2
        }
    }
    Item {
        id: d1; width: 20; height: 20
        transform: Scale {
            id: s2
            property vector3d origin: Qt.vector3d(10, 10, 0)
            xScale: 2; yScale: 2
        }
    }
    Item {
        id: e1; width: 20; height: 20
        transform: Turn { id: t1 }
    }
    Item {
        id: f1; width: 20; height: 20
        transform: Rotation { id: r3; origin.x: 10; origin.y: 10; angle: 90 }
    }
    Item { id: a; width: 20; height: 20; transform: Rotation { id: r; property real angle: 90 } }
    Item { id: b; width: 20; height: 20; transform: Scale { id: s; property real xScale: 3; property int yScale: 3 } }
    Item { id: c; width: 20; height: 20; transform: Translate { id: t; property real x: 7; property string y: "q" } }
    Item { id: d; width: 20; height: 20; property real scale: 2; transformOrigin: Item.TopLeft }
    Item { id: e; width: 20; height: 20; property real rotation: 90; transformOrigin: Item.TopLeft }
    Item { id: f; width: 20; height: 20; transform: Shear { id: h; property real xFactor: 1 } }
    Item { id: g; width: 20; height: 20; transform: Matrix4x4 { id: m; property string matrix: "m" } }
    Item { id: k; width: 20; height: 20; property int transformOrigin: 0; scale: 2 }
    function p(i) { let q = i.mapToItem(root, 20, 10); return Math.round(q.x * 100) / 100 + "," + Math.round(q.y * 100) / 100 }
    Component.onCompleted: {
        result = r1.origin + " " + r1.axis + " " + r1.seen + " " + p(a1)
            + " | " + r2.origin + " " + r2.axis + " " + r2.origin.x + " " + p(b1)
            + " | " + JSON.stringify(s1.origin) + " " + p(c1)
            + " | " + s2.origin + " " + p(d1)
            + " | " + t1.origin + " " + p(e1)
            + " | " + r3.origin + " " + p(f1)
        r1.origin = "w"; r1.axis = 9
        r2.origin = Qt.vector3d(0, 20, 0); r2.origin.x = 5
        s2.origin.y = 3
        result += " || " + r1.origin + " " + r1.axis + " " + r1.seen + " " + p(a1)
            + " | " + r2.origin + " " + p(b1)
            + " | " + s2.origin + " " + p(d1)
            + " | " + log
        result += " ## " + r.angle + " " + p(a) + " | " + s.xScale + " " + s.yScale + " " + p(b) + " | " + t.x + " " + t.y + " " + p(c)
            + " | " + d.scale + " " + p(d) + " | " + e.rotation + " " + p(e) + " | " + h.xFactor + " " + p(f)
            + " | " + m.matrix + " " + p(g) + " | " + k.transformOrigin + " " + p(k)
    }
}
