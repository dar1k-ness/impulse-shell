// Transforms that leave the plane, as Qt 6.12: Shear, a Rotation about another axis than z
// (drawn with the perspective of QMatrix4x4::projectedRotate), `zScale` and `origin.z`, a
// Matrix4x4 with a perspective row, and an item scaled to nothing (its map has no inverse:
// mapFromItem then gives the point back unmapped).
// qt: 0 0 0 0 QVector3D(0, 0, 0) | 25,30 100,0 -5,30 | 10,30 100,45 10,30 | 47.5,35.774 100,57.735 -98.81,87.048 | 10,25.043 98.503,10.166 10,39.667 | 4.349,27.475 78.913,63.401 16.452,27.656 | 8.703,30.147 91.048,0 11.482,29.832 | 23.66,20.981 86.603,-50 -6.34,30.981 | 10,30 100,0 10,30 | 9.715,0 100,0 10,30 | -10,30 -100,0 -10,30 | 7.17,21.51 66.143,0 13.871,41.614 | 9.346,28.037 90.909,0 10.753,32.258 | 0,0 1,2 0,2 1,2 1,2
import QtQuick
Item {
    id: root
    property string result: ""
    function r(v) { return Math.round(v * 1000) / 1000 }
    function pt(p) { return r(p.x) + "," + r(p.y) }
    Shear { id: sh }
    Item { id: a; transform: Shear { xFactor: 0.5 } }
    Item { id: b; transform: Shear { yFactor: 0.5; origin.x: 10; origin.y: 20 } }
    Item { id: c; transform: Shear { xAngle: 45; yAngle: 30; xFactor: 0.25 } }
    Item { id: d; transform: Rotation { axis: Qt.vector3d(1, 0, 0); angle: 60; origin.x: 10; origin.y: 20 } }
    Item { id: e; transform: Rotation { axis: Qt.vector3d(1, 2, 3); angle: 50; origin.x: 10; origin.y: 20 } }
    Item { id: f; transform: Rotation { axis: Qt.vector3d(0, -1, 0); angle: 30 } }
    Item { id: g; transform: Rotation { axis: Qt.vector3d(0, 0, -2); angle: 30 } }
    Item { id: h; transform: Rotation { axis: Qt.vector3d(0, 0, 0); angle: 30 } }
    Item { id: i; transform: Rotation { axis: Qt.vector3d(-1, 0, 0); angle: 90; origin.z: 5 } }
    Item { id: j; transform: Scale { zScale: 3; xScale: -1; origin.z: 5 } }
    Item { id: k; transform: [ Rotation { axis: Qt.vector3d(0, 1, 0); angle: 45 }, Scale { zScale: 3 }, Rotation { axis: Qt.vector3d(1, 0, 0); angle: 45 } ] }
    Item { id: l; transform: Matrix4x4 { matrix: Qt.matrix4x4(1, 0, 0.5, 0,  0, 1, 0, 0,  0.2, 0, 1, 0,  0.001, 0.002, 0, 1) } }
    Item { id: m; scale: 0
 Item { id: m1; x: 5 } }
    Item { id: n; transform: Scale { xScale: 0 }
 Item { id: n1; x: 5 } }
    Component.onCompleted: {
        result += [sh.xFactor, sh.yFactor, sh.xAngle, sh.yAngle, sh.origin].join(" ")
        for (const it of [a, b, c, d, e, f, g, h, i, j, k, l])
            result += " | " + pt(it.mapToItem(root, 10, 30)) + " " + pt(it.mapToItem(root, 100, 0)) + " " + pt(it.mapFromItem(root, 10, 30))
        result += " | " + pt(m1.mapToItem(root, 1, 2)) + " " + pt(m1.mapFromItem(root, 1, 2)) + " " + pt(n1.mapToItem(root, 1, 2)) + " " + pt(n1.mapFromItem(root, 1, 2)) + " " + pt(root.mapToItem(n1, 1, 2))
    }
}
