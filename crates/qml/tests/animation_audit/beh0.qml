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
    property real f: 0
    property real g: 0
    property real h: 0
    property bool go: false
    // Keeps Qt's animation timer ticking between the cases.
    property real keep: 0
    NumberAnimation on keep { from: 0; to: 1; duration: 100000 }
    onAChanged: say("a", r(a), na.running)
    onBChanged: say("b", r(b), r(c), pbc.running)
    onCChanged: say("c", r(b), r(c), pbc.running)
    onDChanged: say("d", r(d), r(e), pde.running)
    onEChanged: say("e", r(d), r(e), pde.running)
    onFChanged: say("f", r(f), nf.running)
    onGChanged: say("g", r(g), ng.running)
    onHChanged: say("h", r(h), sh.running)
    NumberAnimation {
        id: na; target: root; property: "a"; to: 10; duration: 0
        onRunningChanged: root.say("na running", running, root.r(root.a))
        onStarted: root.say("na started", root.r(root.a))
        onStopped: root.say("na stopped", root.r(root.a))
        onFinished: root.say("na finished", root.r(root.a))
    }
    ParallelAnimation {
        id: pbc
        onRunningChanged: root.say("pbc running", running, root.r(root.b), root.r(root.c))
        onStarted: root.say("pbc started")
        onFinished: root.say("pbc finished")
        NumberAnimation { target: root; property: "b"; to: 10; duration: 0 }
        NumberAnimation { target: root; property: "c"; to: 20; duration: 0 }
    }
    PropertyAnimation {
        id: pde; target: root; properties: "d,e"; from: 5; to: 10; duration: 48
        onRunningChanged: root.say("pde running", running, root.r(root.d), root.r(root.e))
        onStarted: root.say("pde started")
        onFinished: root.say("pde finished")
    }
    NumberAnimation {
        id: nf; target: root; property: "f"; to: 10; duration: 0; running: root.go
        onRunningChanged: root.say("nf running", running, root.r(root.f))
        onStarted: root.say("nf started", root.r(root.f))
        onFinished: root.say("nf finished", root.r(root.f))
    }
    onGoChanged: say("go", go, r(f))
    NumberAnimation {
        id: ng; target: root; property: "g"; from: 3; to: 10; duration: 0
        onRunningChanged: root.say("ng running", running, root.r(root.g))
        onStarted: { root.say("ng started", root.r(root.g)) }
        onFinished: root.say("ng finished", root.r(root.g))
    }
    SequentialAnimation {
        id: sh
        onRunningChanged: root.say("sh running", running, root.r(root.h))
        onStarted: root.say("sh started")
        onFinished: root.say("sh finished")
        NumberAnimation { target: root; property: "h"; to: 4; duration: 0 }
        ScriptAction { script: root.say("sh script", root.r(root.h), sh.running) }
        NumberAnimation { target: root; property: "h"; to: 8; duration: 0 }
    }
    function sample() {
        n++
        if (n == 2) { say("start na"); na.start(); say("after na", r(a), na.running) }
        if (n == 4) { say("start pbc"); pbc.start(); say("after pbc", r(b), r(c), pbc.running) }
        if (n == 6) { say("start pde"); pde.start(); say("after pde", r(d), r(e), pde.running) }
        if (n == 12) { say("set go"); go = true; say("after go", r(f), nf.running) }
        if (n == 14) { say("start ng"); ng.start(); say("after ng", r(g), ng.running) }
        if (n == 16) { say("start sh"); sh.start(); say("after sh", r(h), sh.running) }
        if (n == 18) { say("restart na"); a = 0; na.restart(); say("after na", r(a), na.running) }
        say("s", n, r(a), r(b), r(c), r(d), r(e), r(f), r(g), r(h))
    }
}
