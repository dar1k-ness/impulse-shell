import QtQuick
// The order of the writes of one state: literals and bindings, two PropertyChanges, extend.
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
    property real k: 2
    property int ticks: 0
    function snap() { return state + "|" + [a, b, c, d, e, f].map(v => v.toFixed(0)).join(" ") }
    Item { id: other; property real g: 0; property real h: 0
        onGChanged: say("g         ", root.snap(), g, h)
        onHChanged: say("h         ", root.snap(), g, h)
    }
    states: [
        State {
            name: "one"
            PropertyChanges { target: root; a: 1; b: root.k * 10; c: 3; d: root.k + 4 }
            PropertyChanges { target: other; g: 7; h: root.k }
            PropertyChanges { target: root; e: 5 }
        },
        State {
            name: "more"; extend: "one"
            PropertyChanges { target: root; f: 6; a: 11 }
        },
        State {
            name: "less"
            PropertyChanges { target: root; c: 33 }
        }
    ]
    onStateChanged: say("state     ", snap())
    onAChanged: say("a         ", snap())
    onBChanged: say("b         ", b)
    onCChanged: say("c         ", snap())
    onDChanged: say("d         ", d)
    onEChanged: say("e         ", snap())
    onFChanged: say("f         ", snap())
    function sample() {
        ticks++
        say("end       ", snap())
        if (ticks == 1) { state = "one"; say("after one ", snap()) }
        if (ticks == 2) { k = 3; say("after k   ", snap()) }
        if (ticks == 3) { state = "more"; say("after more", snap()) }
        if (ticks == 4) { state = "less"; say("after less", snap()) }
        if (ticks == 5) { state = "one"; say("after one ", snap()) }
        if (ticks == 6) { state = ""; say("after ''  ", snap()) }
    }
}
