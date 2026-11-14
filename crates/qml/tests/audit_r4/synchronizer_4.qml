import QtQuick
import Qt.labs.synchronizer
Item {
    id: root
    property int a: 1
    property int twice: a * 2
    onAChanged: console.log("aChanged", a, "c", other.c, "twice", twice)
    onTwiceChanged: console.log("twiceChanged", twice, "c", other.c)
    Item { id: other; property int c: 10
        property int cc: c + 100
        onCChanged: console.log("cChanged", c, "a", root.a, "cc", cc)
        onCcChanged: console.log("ccChanged", cc, "a", root.a)
        Synchronizer on c { property alias source: root.a }
    }
    Connections { target: root; function onAChanged() { console.log("conn a", root.a, other.c) } }
    Connections { target: other; function onCChanged() { console.log("conn c", root.a, other.c) } }
    Component.onCompleted: {
        console.log("start a", a, "c", other.c);
        a = 5; console.log("a=5 ->", a, other.c);
        other.c = 7; console.log("c=7 ->", a, other.c);
        console.log("END"); Qt.quit();
    }
}
