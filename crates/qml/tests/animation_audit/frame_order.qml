import QtQuick
// Order inside one frame: which animation steps first, and what a handler sees of the others.
Item {
    id: root
    property string log: ""
    function say(...a) { const s = a.join(" "); log += s + "\n"; console.log(s) }
    property real a: 0
    property real b: 0
    property real c: 0
    property real d: 0
    property real e: 0
    property real f: 0
    property real g: 0
    property real h: 0
    property real a2: a * 2
    property int ticks: 0
    function snap() { return [a, b, c, d, e, f, g, h].map(v => v.toFixed(0)).join(" ") }
    NumberAnimation { id: aa; target: root; property: "a"; from: 0; to: 1600; duration: 1600 }
    NumberAnimation { id: ab; target: root; property: "b"; from: 0; to: 1600; duration: 1600 }
    NumberAnimation { id: ac; target: root; property: "c"; from: 0; to: 1600; duration: 1600 }
    ParallelAnimation {
        id: pde
        NumberAnimation { target: root; property: "d"; from: 0; to: 1600; duration: 1600 }
        NumberAnimation { target: root; property: "e"; from: 0; to: 1600; duration: 1600 }
    }
    Behavior on f { NumberAnimation { duration: 1600 } }
    NumberAnimation { id: agh; target: root; properties: "g,h"; from: 0; to: 1600; duration: 1600 }
    Timer { id: tm; interval: 16; repeat: true; onTriggered: say("timer   ", root.snap()) }
    onAChanged: say("a       ", snap())
    onA2Changed: say("a2      ", snap(), a2)
    onBChanged: say("b       ", snap())
    onCChanged: say("c       ", snap())
    onDChanged: say("d       ", snap())
    onEChanged: say("e       ", snap())
    onFChanged: say("f       ", snap())
    onGChanged: say("g       ", snap())
    onHChanged: say("h       ", snap())
    Component.onCompleted: {
        ab.start(); tm.start(); aa.start(); f = 1600; pde.start(); agh.start(); ac.start();
        say("completed", snap())
    }
    function sample() {
        ticks++
        say("end     ", snap())
        if (ticks == 2) { ab.restart(); say("b restarted") }
        if (ticks == 4) { aa.pause(); say("a paused") }
        if (ticks == 5) { aa.resume(); say("a resumed") }
        if (ticks == 7) { f = 0; say("f retargeted") }
        if (ticks == 9) { ac.stop(); ac.start(); say("c stopped and started") }
    }
}
