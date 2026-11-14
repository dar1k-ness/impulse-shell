// A handler that asks for a component with Component.Asynchronous and then stays busy. Qt's
// loader thread posts "the document is here" while the handler still runs, before the call and
// the Timer it posts afterwards: S C T. Here the document arrives after the handler returned:
// C T S (docs/qt-differences.md).
import QtQuick
Item {
    id: root
    property string result: ""
    property var got: []
    property var keep: []
    function hit(l) { got.push(l) }
    function busy(ms) { var t = Date.now(); while (Date.now() - t < ms) ; }
    Timer { id: t0; interval: 0; onTriggered: root.hit("T") }
    Timer { interval: 100; running: true; onTriggered: { root.result = root.got.join(" "); Qt.quit() } }
    Component.onCompleted: {
        var c = Qt.createComponent("Fresh.qml", Component.Asynchronous)
        c.statusChanged.connect(function() { root.hit("S") })
        keep.push(c)
        busy(150)
        Qt.callLater(function() { root.hit("C") })
        t0.start()
    }
}
