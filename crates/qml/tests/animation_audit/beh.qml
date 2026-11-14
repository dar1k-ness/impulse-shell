import QtQuick
Item {
    id: root
    property string log: ""
    function say(...a) { const s = a.join(" "); log += s + "\n"; console.log(s) }
    function r(v) { return Math.round(v * 1000) / 1000 }
    property int n: 0
    property real a: 0
    property real b: 0
    property real c: 0
    property real d: 0
    property real e: 0
    property real src: 0
    property real f: src
    // Keeps Qt's animation timer ticking between the cases.
    property real keep: 0
    NumberAnimation on keep { from: 0; to: 1; duration: 100000 }
    onAChanged: say("a", r(a), sa.running)
    onBChanged: say("b", r(b), nb.running)
    onCChanged: say("c", r(c), nc.running)
    onDChanged: say("d", r(d), sd.running)
    onEChanged: say("e", r(e), se.running)
    onFChanged: say("f", r(f), nf.running)
    Behavior on a {
        SpringAnimation { id: sa; onRunningChanged: root.say("sa running", running, root.r(root.a)) }
    }
    Behavior on b {
        NumberAnimation {
            id: nb; duration: 100
            onRunningChanged: root.say("nb running", running, root.r(root.b))
            onStarted: root.say("nb started")
            onStopped: root.say("nb stopped")
            onFinished: root.say("nb finished")
        }
    }
    Behavior on c {
        NumberAnimation { id: nc; duration: 0; onRunningChanged: root.say("nc running", running, root.r(root.c)) }
    }
    Behavior on d {
        SmoothedAnimation { id: sd; velocity: 200; onRunningChanged: root.say("sd running", running, root.r(root.d)) }
    }
    Behavior on e {
        SequentialAnimation {
            id: se
            onRunningChanged: root.say("se running", running, root.r(root.e))
            PauseAnimation { duration: 32 }
            NumberAnimation { duration: 48 }
        }
    }
    Behavior on f {
        NumberAnimation { id: nf; duration: 64; onRunningChanged: root.say("nf running", running, root.r(root.f)) }
    }
    function sample() {
        n++
        if (n == 2) { say("set a"); a = 10; say("after a", r(a), sa.running) }
        if (n == 4) { say("set b"); b = 10; say("after b", r(b), nb.running) }
        if (n == 7) { say("set b again"); b = 20; say("after b", r(b), nb.running) }
        if (n == 9) { say("set b same"); b = 20; say("after b", r(b), nb.running) }
        if (n == 18) { say("set c"); c = 10; say("after c", r(c), nc.running) }
        if (n == 20) { say("set d"); d = 10; say("after d", r(d), sd.running) }
        if (n == 22) { say("set d again"); d = 4; say("after d", r(d), sd.running) }
        if (n == 30) { say("set e"); e = 10; say("after e", r(e), se.running) }
        if (n == 40) { say("set src"); src = 10; say("after src", r(f), nf.running) }
        if (n == 48) { say("set a twice"); a = 20; a = 30; say("after a", r(a), sa.running) }
        if (n == 50) { say("set a to itself"); a = 30; say("after a", r(a), sa.running) }
        say("s", n, r(a), r(b), r(c), r(d), r(e), r(f))
    }
}
