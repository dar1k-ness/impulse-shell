import QtQuick
import Qt.labs.synchronizer
Item {
    id: root
    property int a: 1
    Item { id: other; property int c: 10
        Synchronizer on c { property alias source: root.a }
    }
    function f() { other.c = 7; a = 5 }
    Component.onCompleted: { f(); console.log("in handler", a, other.c); t.start() }
    Timer { id: t; interval: 30; onTriggered: { console.log("later", root.a, other.c); console.log("END"); Qt.quit() } }
}
