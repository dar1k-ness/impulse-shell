// What the properties of the transform types read as in Qt 6.12: `origin` and `axis` are
// QVector3D values also when their fields were set one by one (`origin.x: 50`), and a write to
// a field of the value goes back to the transform.
// qt: QVector3D(50, 25, 0) 50 QVector3D(0, 1, 0) QVector3D(5, 0, 0) QVector3D(1, 0, 0) QVector3D(1, 2, 3) QMatrix4x4(2, 0.5, 0, 30, -0.5, 1, 0, 40, 0, 0, 1, 0, 0, 0, 0, 1) | QVector3D(50, 7, 0) QVector3D(0, 1, 4) QVector3D(9, 9, 9)
import QtQuick
Item {
    id: root
    property string result: ""
    Scale { id: s; origin.x: 50; origin.y: 25; xScale: 2 }
    Rotation { id: r; axis { x: 0; y: 1; z: 0 } angle: 60; origin.x: 5 }
    Rotation { id: r2; axis: Qt.vector3d(1, 0, 0); origin: Qt.vector3d(1, 2, 3) }
    Matrix4x4 { id: m; matrix: Qt.matrix4x4(2, 0.5, 0, 30,  -0.5, 1, 0, 40,  0, 0, 1, 0,  0, 0, 0, 1) }
    Component.onCompleted: {
        result += s.origin + " " + s.origin.x + " " + r.axis + " " + r.origin + " " + r2.axis + " " + r2.origin + " " + m.matrix
        r.axis.z = 4; s.origin.y = 7; r2.origin = Qt.vector3d(9, 9, 9)
        result += " | " + s.origin + " " + r.axis + " " + r2.origin
    }
}
