import QtQuick
// A state switch without a Transition: a StateGroup, and a state entered from a handler.
Item {
    id: root
    property string log: ""
    function say(...a) { const s = a.join(" "); log += s + "\n"; console.log(s) }
    property real p: 0
    property real q: 0
    property real r: 0
    property real sum: p + q
    property int ticks: 0
    function snap() { return state + "/" + sg.state + "|" + [p, q, r].map(v => v.toFixed(0)).join(" ") }
    states: [
        State {
            name: "on"
            PropertyChanges { target: root; p: 80; q: 160 }
        },
        State {
            name: "two"
            PropertyChanges { target: root; q: 7; p: 3 }
        }
    ]
    StateGroup {
        id: sg
        states: State {
            name: "g"
            PropertyChanges { target: root; r: 33 }
        }
        onStateChanged: say("sg.state  ", root.snap())
    }
    onStateChanged: say("state     ", snap())
    onPChanged: { say("p         ", snap()); if (p == 3) sg.state = "g" }
    onQChanged: say("q         ", snap())
    onRChanged: say("r         ", snap())
    onSumChanged: say("sum       ", snap(), sum)
    function sample() {
        ticks++
        say("end       ", snap())
        if (ticks == 1) { state = "on"; say("after on  ", snap()) }
        if (ticks == 2) { state = ""; say("after ''  ", snap()) }
        if (ticks == 3) { state = "two"; say("after two ", snap()) }
        if (ticks == 4) { state = "on"; say("after on  ", snap()) }
        if (ticks == 5) { state = ""; sg.state = ""; say("after ''  ", snap()) }
    }
}
