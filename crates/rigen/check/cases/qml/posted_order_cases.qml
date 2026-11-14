// A Timer with interval 0 and `Qt.callLater` share one queue of posted events (QQmlTimer posts
// QEvent_Triggered from inside `start()`, QQmlDelayedCallQueue one event for the calls of a
// turn). Each case is one handler and what it left runs before the next case:
//   twoTimers        a call between the starts of two Timers keeps its place
//   repeat0          a repeating Timer with interval 0 never triggers
//   onStart0         `triggeredOnStart` with interval 0 triggers once
//   laterInLater     what a posted call posts runs after what stood in the queue
//   restartInHandler a Timer started again by its handler takes the tail each time
//   t0_t1            a Timer with interval 1 waits for the clock
//   loadedAsync      the document of an asynchronous component arrives after the handler
//   stop0            `stop()` leaves the posted event empty
//   stopStart0       started again, the Timer triggers at the place of its first event
//   restart0, startTwice0  `restart()` and a second `start()` do not move it
//   bindTC, bindCT   `running` set through a binding posts as `start()` does
//   int0to1          interval 0 made 1: the posted event finds the Timer counting and is empty
//   int1to0          interval 1 made 0 posts
//   int0to1to0       back to 0: the first event triggers
//   inTimer          a call and a start made in a Timer's handler go to the tail
//   inLater          a start and a call made in a posted call go to the tail
//   chainVsT1        a chain of posted events runs before a Timer of 1 ms whose time has come
//   syncVsT          an incubator asked with Qt.Synchronous is Ready at once
// The line below is Qt 6.12's `result` (crates/qml/tests/posted_order.rs asks Qt again).
// qt: twoTimers>T C U;repeat0>C;onStart0>O C;laterInLater>C1 C2 T;restartInHandler>A0 C c1 A1 c2 A2;t0_t1>T C T1;loadedAsync>status2 C S;stop0>C;stopStart0>T C;restart0>T C;startTwice0>T C;bindTC>B C;bindCT>C B;int0to1>C U W;int1to0>W C U;int0to1to0>W C U;inTimer>A V c U;inLater>C1 V T C2;chainVsT1>A0 c1 A1 c2 A2 T1;syncVsT>i1 T C
import QtQuick
Item {
    id: root
    property string result: ""
    property var got: []
    property var done: []
    property int k: -1
    property var keep: []
    property bool flagA: false
    property bool flagB: false
    property var names: ["twoTimers", "repeat0", "onStart0", "laterInLater", "restartInHandler", "t0_t1", "loadedAsync",
        "stop0", "stopStart0", "restart0", "startTwice0", "bindTC", "bindCT", "int0to1", "int1to0", "int0to1to0",
        "inTimer", "inLater", "chainVsT1", "syncVsT"]
    function hit(l) { got.push(l) }
    function later(l) { Qt.callLater(function() { root.hit(l) }) }
    function busy(ms) { var t = Date.now(); while (Date.now() - t < ms) ; }
    function round() {
        if (k >= 0) {
            rep.stop(); flagA = false; flagB = false
            done.push(names[k] + ">" + got.join(" "))
        }
        k++
        if (k >= names.length) {
            result = done.join(";")
            Qt.quit()
            return
        }
        got = []
        switch (names[k]) {
        case "twoTimers": t0.start(); later("C"); u0.start(); break
        case "repeat0": rep.start(); later("C"); break
        case "onStart0": os.start(); later("C"); break
        case "laterInLater": Qt.callLater(function() { root.hit("C1"); root.later("C2"); t0.start() }); break
        case "restartInHandler": again.n = 0; again.start(); later("C"); break
        case "t0_t1": t1.start(); t0.start(); later("C"); break
        case "loadedAsync":
            var c = Qt.createComponent("posted_order/Late.qml", Component.Asynchronous)
            c.statusChanged.connect(function() { root.hit("S") })
            keep.push(c)
            hit("status" + c.status); later("C"); break
        case "stop0": t0.start(); t0.stop(); later("C"); break
        case "stopStart0": t0.start(); t0.stop(); later("C"); t0.start(); break
        case "restart0": t0.start(); later("C"); t0.restart(); break
        case "startTwice0": t0.start(); later("C"); t0.start(); break
        case "bindTC": flagA = true; later("C"); break
        case "bindCT": later("C"); flagB = true; break
        case "int0to1": tv.interval = 0; tv.start(); tv.interval = 1; later("C"); u0.start(); break
        case "int1to0": tv.interval = 1; tv.start(); tv.interval = 0; later("C"); u0.start(); break
        case "int0to1to0": tv.interval = 0; tv.start(); later("C"); tv.interval = 1; tv.interval = 0; u0.start(); break
        case "inTimer": a2.start(); v0.start(); break
        case "inLater": Qt.callLater(function() { root.hit("C1"); t0.start(); root.later("C2") }); v0.start(); break
        case "chainVsT1": t1.start(); busy(10); again.n = 0; again.start(); break
        case "syncVsT":
            t0.start()
            var i = ready.incubateObject(root, {}, Qt.Synchronous)
            hit("i" + i.status); later("C"); break
        }
        next.start()
    }
    Component { id: ready; QtObject { } }
    Timer { id: t0; interval: 0; onTriggered: root.hit("T") }
    Timer { id: u0; interval: 0; onTriggered: root.hit("U") }
    Timer { id: v0; interval: 0; onTriggered: root.hit("V") }
    Timer { id: tv; interval: 0; onTriggered: root.hit("W") }
    Timer { id: t1; interval: 1; onTriggered: root.hit("T1") }
    Timer { id: rep; interval: 0; repeat: true; onTriggered: if (root.got.length < 6) root.hit("R") }
    Timer { id: os; interval: 0; triggeredOnStart: true; onTriggered: root.hit("O") }
    Timer { id: bA; interval: 0; running: root.flagA; onTriggered: root.hit("B") }
    Timer { id: bB; interval: 0; running: root.flagB; onTriggered: root.hit("B") }
    Timer { id: a2; interval: 0; onTriggered: { root.hit("A"); root.later("c"); u0.start() } }
    Timer { id: again; interval: 0; property int n: 0; onTriggered: { root.hit("A" + n); if (++n < 3) { root.later("c" + n); start() } } }
    Timer { id: next; interval: 100; onTriggered: root.round() }
    Component.onCompleted: next.start()
}
