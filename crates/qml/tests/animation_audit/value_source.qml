import QtQuick
// `<Animation> on <property>` runs by itself unless `running: false`: each type, loops, alwaysRunToEnd, in a Loader and a delegate.
Item {
    id: root
    property string log: ""
    function say(...a) { const s = a.join(" "); log += s + "\n"; console.log(s) }
    property int ticks: 0
    property real a: 0
    property real b: 0
    property real c: 0
    property real d: 0
    property real e: 0
    property real f: 0
    property color col: "#000000"
    property real rot: 0
    property real sm: 0
    property real pa: 0
    property real sq: 0
    property real par: 0
    property real par2: 0
    property real beh: 0
    property real late: 0
    property real g: 0
    property vector3d v3
    property real pact: 0
    property bool go: false
    NumberAnimation on a { id: na; from: 0; to: 100; duration: 160
        onStarted: root.say("na started", running); onRunningChanged: root.say("na running", running); onFinished: root.say("na finished", root.a) }
    NumberAnimation on b { id: nb; running: false; from: 0; to: 100; duration: 160 }
    NumberAnimation on c { id: nc; from: 0; to: 100; duration: 64; loops: Animation.Infinite }
    NumberAnimation on d { id: nd; from: 0; to: 100; duration: 64; loops: 2 }
    NumberAnimation on e { id: ne; from: 0; to: 100; duration: 160; alwaysRunToEnd: true }
    NumberAnimation on f { id: nf; to: 100; duration: 160 }
    ColorAnimation on col { id: ca; to: "#804020"; duration: 160 }
    RotationAnimation on rot { id: ra; from: 350; to: 10; direction: RotationAnimation.Clockwise; duration: 160 }
    SmoothedAnimation on sm { id: sa; to: 100; velocity: 400 }
    PropertyAnimation on pa { id: ppa; to: 100; duration: 160 }
    SequentialAnimation on sq {
        id: sqa
        NumberAnimation { to: 50; duration: 64 }
        PauseAnimation { duration: 32 }
        NumberAnimation { to: 100; duration: 64 }
    }
    ParallelAnimation on par {
        id: para
        NumberAnimation { to: 100; duration: 160 }
        NumberAnimation { target: root; property: "par2"; to: 100; duration: 160 }
    }
    Behavior on beh { NumberAnimation { id: ba; duration: 160 } }
    NumberAnimation on late { id: nl; running: false; from: 0; to: 100; duration: 160 }
    NumberAnimation on g { id: ng; running: root.go; from: 0; to: 100; duration: 160 }
    Vector3dAnimation on v3 { id: va; to: Qt.vector3d(100, 200, 300); duration: 160 }
    PropertyAction on pact { id: pac; value: 7 }
    Loader { id: ld; sourceComponent: Component { Item { property real v: 0; property alias an: lan
        NumberAnimation on v { id: lan; from: 0; to: 100; duration: 160 } } } }
    Repeater { id: rep; model: 2
        Item { property real v: 0; property alias an: dan
            NumberAnimation on v { id: dan; from: 0; to: 100 * (index + 1); duration: 160 } } }
    function r(x) { return x ? "1" : "0" }
    function flags() {
        return [na, nb, nc, nd, ne, nf, ca, ra, sa, ppa, sqa, para, ba, nl, ng, va, pac, ld.item.an, rep.itemAt(0).an, rep.itemAt(1).an].map(x => r(x.running)).join("")
    }
    function vals() {
        return [a, b, c, d, e, f, rot, sm, pa, sq, par, par2, beh, late, g, pact, ld.item.v, rep.itemAt(0).v, rep.itemAt(1).v].map(v => v.toFixed(1)).join(" ") + " " + col
    }
    Component.onCompleted: say("completed", flags(), vals())
    function sample() {
        ticks++
        say("s", ticks, flags(), vals())
        if (ticks == 3) { beh = 100; nl.running = true; ne.stop(); say("after", flags(), vals()) }
        if (ticks == 5) { go = true; say("after go", flags()) }
    }
}
