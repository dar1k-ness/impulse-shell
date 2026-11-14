// The order of what one handler leaves for later turns of the event loop, for the 24 orders
// of asking: C a `Qt.callLater` call, T the start of a Timer with interval 0, S `statusChanged`
// of a component asked for with `Component.Asynchronous`, I the object of
// `Component.incubateObject`. The call and the Timer are posted events and come in the order
// they were asked for; the document of the component arrives after the handler returned; the
// incubation controller of the window works after them.
// The line below is Qt 6.12's `result` with the document in a QQuickView, which gives the
// engine its incubation controller (crates/qml/tests/posted_order.rs asks Qt again).
// qt: CTSI>CTSI CTIS>CTSI CSTI>CTSI CSIT>CTSI CITS>CTSI CIST>CTSI TCSI>TCSI TCIS>TCSI TSCI>TCSI TSIC>TCSI TICS>TCSI TISC>TCSI SCTI>CTSI SCIT>CTSI STCI>TCSI STIC>TCSI SICT>CTSI SITC>TCSI ICTS>CTSI ICST>CTSI ITCS>TCSI ITSC>TCSI ISCT>CTSI ISTC>TCSI
import QtQuick
Item {
    id: root
    property string result: ""
    property var perms: ["CTSI", "CTIS", "CSTI", "CSIT", "CITS", "CIST", "TCSI", "TCIS", "TSCI", "TSIC", "TICS", "TISC",
        "SCTI", "SCIT", "STCI", "STIC", "SICT", "SITC", "ICTS", "ICST", "ITCS", "ITSC", "ISCT", "ISTC"]
    property int k: -1
    property var got: []
    property var done: []
    property var keep: []
    function hit(l) { got.push(l) }
    function later(l) { Qt.callLater(function() { root.hit(l) }) }
    function round() {
        if (k >= 0)
            done.push(perms[k] + ">" + got.join(""))
        k++
        if (k >= perms.length) {
            result = done.join(" ")
            Qt.quit()
            return
        }
        got = []
        var p = perms[k]
        for (var i = 0; i < 4; i++) {
            switch (p[i]) {
            case "C": later("C"); break
            case "T": t0.start(); break
            case "S":
                var c = Qt.createComponent("posted_order/Fresh" + (k < 10 ? "0" : "") + k + ".qml", Component.Asynchronous)
                if (c.status !== Component.Loading)
                    root.hit("S!" + c.status)
                c.statusChanged.connect(function() { root.hit("S") })
                keep.push(c)
                break
            case "I":
                var inc = ready.incubateObject(root)
                if (inc.status !== Component.Loading)
                    root.hit("I!")
                inc.onStatusChanged = function(s) { root.hit("I") }
                keep.push(inc)
                break
            }
        }
        next.start()
    }
    Component { id: ready; QtObject { } }
    Timer { id: t0; interval: 0; onTriggered: root.hit("T") }
    Timer { id: next; interval: 100; onTriggered: root.round() }
    Component.onCompleted: next.start()
}
