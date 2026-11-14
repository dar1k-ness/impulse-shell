// A Timer declared running posts from its componentComplete, which Qt runs last-created first
// and before any `Component.onCompleted`: P and O (QEvent_MaybeTick of `triggeredOnStart`), B
// and A (QEvent_Triggered of interval 0) stand in the queue in that order, behind what the
// Loader created after them posted while it loaded (D, and the event of the calls c0 c2 c1),
// and before L, started by `Component.onCompleted`.
// The line below is Qt 6.12's `result` (crates/qml/tests/posted_order.rs asks Qt again).
// qt: D c0 c2 c1 P O B A L P
import QtQuick
Item {
    id: root
    property string result: ""
    property var got: []
    function hit(l) { got.push(l) }
    Timer { interval: 0; running: true; onTriggered: root.hit("A") }
    Item { Timer { interval: 0; running: true; onTriggered: root.hit("B") }
           Component.onCompleted: Qt.callLater(function() { root.hit("c1") }) }
    Timer { interval: 0; running: true; triggeredOnStart: true; onTriggered: root.hit("O") }
    Timer { interval: 50; running: true; triggeredOnStart: true; onTriggered: root.hit("P") }
    Timer { id: late; interval: 0; onTriggered: root.hit("L") }
    Loader { sourceComponent: Item { Timer { interval: 0; running: true; onTriggered: root.hit("D") }
        Component.onCompleted: Qt.callLater(function() { root.hit("c0") }) } }
    Timer { interval: 100; running: true; onTriggered: { root.result = root.got.join(" "); Qt.quit() } }
    Component.onCompleted: { Qt.callLater(function() { root.hit("c2") }); late.start() }
}
