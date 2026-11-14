import QtQuick
import Qt.labs.synchronizer
Item {
    id: root
    property int a: 1
    onAChanged: console.log("aChanged", a, "c", other.c)
    Item { id: other; property int c: 10
        onCChanged: console.log("cChanged", c, "a", root.a)
        Synchronizer on c { property alias source: root.a }
    }
    Component.onCompleted: {
        console.log("start", a, other.c);
        a = 5; console.log("a=5 ->", a, other.c);
        other.c = 7; console.log("c=7 ->", a, other.c);
        console.log("END"); Qt.quit()
    }
}
