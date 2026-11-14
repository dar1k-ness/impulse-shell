import QtQuick
Item {
    id: root
    property string log: ""
    function say(...a) { const s = a.join(" "); log += s + "\n"; console.log(s) }
    property int n: -10    // NumberAnimation: reals, the int property rounds them
    property int pa: -10   // PropertyAnimation: ints, cut off
    property int sm: -10   // SmoothedAnimation
    property int sp: 10    // SpringAnimation
    property int be: 0     // NumberAnimation in a Behavior
    NumberAnimation { id: an; target: root; property: "n"; from: -10; to: 10; duration: 400 }
    PropertyAnimation { id: ap; target: root; property: "pa"; from: -10; to: 10; duration: 400 }
    SmoothedAnimation { id: asm; target: root; property: "sm"; to: 10; velocity: 50 }
    SpringAnimation { id: asp; target: root; property: "sp"; to: -10; spring: 2; damping: 0.2 }
    Behavior on be { NumberAnimation { duration: 400 } }
    Component.onCompleted: { an.start(); ap.start(); asm.start(); asp.start(); be = -7 }
    function sample() { say("s", n, pa, sm, sp, be) }
}
