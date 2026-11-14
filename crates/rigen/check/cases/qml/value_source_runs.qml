// `<Animation> on <property>` runs by itself when its component completes, unless
// `running: false` is written; a `PropertyAction on` has acted by then.
// qt: s 1011111111110010101111 7 350
import QtQuick
Item {
    id: root
    property string result: ""
    property real a: 0
    property real b: 0
    property real c: 0
    property real d: 0
    property real e: 0
    property color col: "#000000"
    property real rot: 0
    property real sm: 0
    property real sp: 0
    property real pa: 0
    property real sq: 0
    property real par: 0
    property real beh: 0
    property real g: 0
    property bool go: false
    property vector3d v3
    property real pact: 0
    NumberAnimation on a { id: na; from: 0; to: 100; duration: 160; onStarted: root.result += "s" }
    NumberAnimation on b { id: nb; running: false; from: 0; to: 100; duration: 160; onStarted: root.result += "!" }
    NumberAnimation on c { id: nc; to: 100; duration: 64; loops: Animation.Infinite }
    NumberAnimation on d { id: nd; to: 100; duration: 64; loops: 2 }
    NumberAnimation on e { id: ne; to: 100; duration: 160; alwaysRunToEnd: true }
    ColorAnimation on col { id: ca; to: "#804020"; duration: 160 }
    RotationAnimation on rot { id: ra; from: 350; to: 10; direction: RotationAnimation.Clockwise; duration: 160 }
    SmoothedAnimation on sm { id: sa; to: 100; velocity: 400 }
    SpringAnimation on sp { id: spa; to: 100; spring: 5; damping: 0.5 }
    PropertyAnimation on pa { id: ppa; to: 100; duration: 160 }
    SequentialAnimation on sq {
        id: sqa
        NumberAnimation { to: 50; duration: 64 }
        PauseAnimation { duration: 32 }
    }
    ParallelAnimation on par {
        id: para
        NumberAnimation { to: 100; duration: 160 }
    }
    Behavior on beh { NumberAnimation { id: ba; duration: 160 } }
    NumberAnimation on g { id: ng; running: root.go; to: 100; duration: 160 }
    Vector3dAnimation on v3 { id: va; to: Qt.vector3d(100, 200, 300); duration: 160 }
    PropertyAction on pact { id: pac; value: 7 }
    Item {
        XAnimator on x { id: xa; from: 0; to: 100; duration: 160 }
        OpacityAnimator on opacity { id: oa; running: false; from: 0; to: 1; duration: 160 }
        RotationAnimator on rotation { id: rta; from: 0; to: 90; duration: 160; loops: Animation.Infinite }
    }
    Loader { id: ld; sourceComponent: Component {
        Item { property real v: 0; property alias an: lan
            NumberAnimation on v { id: lan; to: 100; duration: 160 } } } }
    Repeater { id: rep; model: 2
        Item { property real v: 0; property alias an: dan
            NumberAnimation on v { id: dan; to: 100; duration: 160 } } }
    Component.onCompleted: result += " " + [na, nb, nc, nd, ne, ca, ra, sa, spa, ppa, sqa, para, ba, ng, va, pac, xa, oa, rta,
        ld.item.an, rep.itemAt(0).an, rep.itemAt(1).an].map(x => x.running ? "1" : "0").join("") + " " + pact + " " + rot
}
