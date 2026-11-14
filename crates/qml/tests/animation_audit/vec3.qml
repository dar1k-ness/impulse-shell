import QtQuick
Item {
    id: root
    property string log: ""
    function say(...a) { const s = a.join(" "); log += s + "\n"; console.log(s) }
    property int n: 0
    property vector3d a                               // from and to, linear
    property vector3d b: Qt.vector3d(1, 2, 3)         // no from, InOutQuad
    property vector3d c                               // in a Behavior
    property vector3d d                               // in a Transition, no property named
    property real dn: 0                               // a number of the same state change
    property vector3d e                               // Vector3dAnimation on e
    property vector3d f: Qt.vector3d(0.1, 0.2, 0.3)   // to thirds: float arithmetic
    property vector3d g                               // to written as a string
    property var h: Qt.vector3d(1, 1, 1)              // a var property
    property vector3d p                               // a PropertyAnimation
    property vector2d v2
    property vector4d v4
    property point pt
    property size sz
    property rect rc
    property quaternion qu
    function v(x) { return x.x + "," + x.y + "," + x.z }
    Vector3dAnimation { id: va; target: root; property: "a"; from: Qt.vector3d(0, 0, 0); to: Qt.vector3d(10, 20, 30); duration: 160 }
    Vector3dAnimation { id: vb; target: root; property: "b"; to: Qt.vector3d(11, -2, 3); duration: 200; easing.type: Easing.InOutQuad }
    Behavior on c { Vector3dAnimation { duration: 96 } }
    states: State { name: "on"; PropertyChanges { root.d: Qt.vector3d(4, 8, 16); root.dn: 5 } }
    transitions: Transition { Vector3dAnimation { duration: 128 } }
    Vector3dAnimation on e { to: Qt.vector3d(7, 7, 7); duration: 112 }
    Vector3dAnimation { id: vf; target: root; property: "f"; to: Qt.vector3d(1 / 3, 2 / 3, 1); duration: 240 }
    Vector3dAnimation { id: vg; target: root; property: "g"; to: "1,2,3"; duration: 80 }
    Vector3dAnimation { id: vh; target: root; property: "h"; to: Qt.vector3d(5, 6, 7); duration: 80 }
    PropertyAnimation { id: pp; target: root; property: "p"; to: Qt.vector3d(3, 2, 1); duration: 160 }
    PropertyAnimation { id: p2; target: root; property: "v2"; to: Qt.vector2d(1, 2); duration: 160 }
    PropertyAnimation { id: p4; target: root; property: "v4"; to: Qt.vector4d(1, 2, 3, 4); duration: 160 }
    PropertyAnimation { id: ppt; target: root; property: "pt"; to: Qt.point(10, 20); duration: 160 }
    PropertyAnimation { id: psz; target: root; property: "sz"; from: Qt.size(0, 0); to: Qt.size(30, 40); duration: 160 }
    PropertyAnimation { id: prc; target: root; property: "rc"; to: Qt.rect(1, 2, 30, 40); duration: 160 }
    PropertyAnimation { id: pqu; target: root; property: "qu"; to: Qt.quaternion(0, 1, 0, 0); duration: 160 }
    function sample() {
        n++
        say("s", v(a), "|", v(b), "|", v(c), "|", v(d), dn, "|", v(e))
        say("t", v(f), "|", v(g), "|", String(h), "|", v(p))
        say("u", v2.x + "," + v2.y, "|", v4.x + "," + v4.y + "," + v4.z + "," + v4.w, "|", pt.x + "," + pt.y, "|", sz.width + "," + sz.height,
            "|", rc.x + "," + rc.y + "," + rc.width + "," + rc.height, "|", qu.scalar + "," + qu.x + "," + qu.y + "," + qu.z)
        if (n == 3) { say("retarget"); c = Qt.vector3d(-6, 0, 6) }
    }
    Component.onCompleted: {
        va.start(); vb.start(); c = Qt.vector3d(6, 12, 18); state = "on"; vf.start(); vg.start(); vh.start()
        pp.start(); p2.start(); p4.start(); ppt.start(); psz.start(); prc.start(); pqu.start()
    }
}
