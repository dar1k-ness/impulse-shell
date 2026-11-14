// posted_order_declared.qml of crates/rigen/check with the Loader declared before the Timers.
// Qt completes it after them (last-created first), so what it posts while loading stands
// behind theirs: P O B A D c0 c2 c1 L P. Here a Loader loads before the Timers of its
// component are complete: D c0 c2 c1 P O B A L P.
import QtQuick
Item {
    id: root
    property string result: ""
    property var got: []
    function hit(l) { got.push(l) }
    Loader { sourceComponent: Item { Timer { interval: 0; running: true; onTriggered: root.hit("D") }
        Component.onCompleted: Qt.callLater(function() { root.hit("c0") }) } }
    Timer { interval: 0; running: true; onTriggered: root.hit("A") }
    Item { Timer { interval: 0; running: true; onTriggered: root.hit("B") }
           Component.onCompleted: Qt.callLater(function() { root.hit("c1") }) }
    Timer { interval: 0; running: true; triggeredOnStart: true; onTriggered: root.hit("O") }
    Timer { interval: 50; running: true; triggeredOnStart: true; onTriggered: root.hit("P") }
    Timer { id: late; interval: 0; onTriggered: root.hit("L") }
    Timer { interval: 100; running: true; onTriggered: { root.result = root.got.join(" "); Qt.quit() } }
    Component.onCompleted: { Qt.callLater(function() { root.hit("c2") }); late.start() }
}
