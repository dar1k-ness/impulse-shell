import QtQuick
Item {
    id: root
    property string log: ""
    function say(...a) { const s = a.join(" "); log += s + "\n"; console.log(s) }
    property real lin: 0
    property real a: 350      // explicit from/to, Clockwise
    property real b: 10       // explicit, Counterclockwise
    property real c: 0        // explicit, Shortest, to 270
    property real d: 0        // default direction (Numerical), to 270
    property real e: 100      // Shortest exactly 180 apart
    property real f: 100      // Shortest -180 apart
    property real g: 30       // Clockwise, same value mod 360: 30 -> 390
    property real h: 30       // Clockwise from > to by more than 360: 800 -> 10
    property real rotation2: 0
    Item { id: it; rotation: 350; property real angle: 20; property real other: 5
        states: State { name: "s"; PropertyChanges { target: it; rotation: 10; angle: 300; other: 50 } }
        transitions: Transition { RotationAnimation { direction: RotationAnimation.Shortest; duration: 160 } }
    }
    RotationAnimation { id: ra; target: root; property: "a"; from: 350; to: 10; direction: RotationAnimation.Clockwise; duration: 160 }
    RotationAnimation { id: rb; target: root; property: "b"; from: 10; to: 350; direction: RotationAnimation.Counterclockwise; duration: 160 }
    RotationAnimation { id: rc; target: root; property: "c"; to: 270; direction: RotationAnimation.Shortest; duration: 160 }
    RotationAnimation { id: rd; target: root; property: "d"; to: 270; duration: 160 }
    RotationAnimation { id: re; target: root; property: "e"; to: 280; direction: RotationAnimation.Shortest; duration: 160 }
    RotationAnimation { id: rf; target: root; property: "f"; to: -80; direction: RotationAnimation.Shortest; duration: 160 }
    RotationAnimation { id: rg; target: root; property: "g"; to: 390; direction: RotationAnimation.Clockwise; duration: 160 }
    RotationAnimation { id: rh; target: root; property: "h"; from: 800; to: 10; direction: RotationAnimation.Clockwise; duration: 160; easing.type: Easing.OutCubic }
    NumberAnimation { id: ref; target: root; property: "lin"; from: 0; to: 1; duration: 160 }
    function sample() { say("s", lin.toFixed(3), a.toFixed(3), b.toFixed(3), c.toFixed(3), d.toFixed(3), e.toFixed(3), f.toFixed(3), g.toFixed(3), h.toFixed(3), it.rotation.toFixed(3), it.angle.toFixed(3), it.other) }
    Component.onCompleted: { ra.start(); rb.start(); rc.start(); rd.start(); re.start(); rf.start(); rg.start(); rh.start(); it.state = "s"; ref.start(); say("start", a, b, c, d, e, f, g, h, it.rotation, it.angle, it.other, rc.direction, rd.direction) }
}
