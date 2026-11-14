import QtQuick
import QtQml.Models
Item {
    id: root
    property int n: 1
    property QtObject o: QtObject { property int v: 1 }
    Item { property int a: root.n + root.o.v; onAChanged: console.log("s1", a) }
    Item { property int a: root.n + root.o.v; onAChanged: console.log("s2", a)
        Item { property int a: root.n + root.o.v; onAChanged: console.log("s2.c", a) } }
    Repeater {
        model: 3
        Item { property int a: root.n + root.o.v; onAChanged: console.log("rp", index, a)
            Item { property int a: root.n + root.o.v; onAChanged: console.log("rp.c", index, a) } }
    }
    Loader { sourceComponent: Component { Item { property int a: root.n + root.o.v; onAChanged: console.log("l1", a) } } }
    Loader { sourceComponent: Component { Item { property int a: root.n + root.o.v; onAChanged: console.log("l2", a) } } }
    Instantiator {
        model: 2
        QtObject { property int a: root.n + root.o.v; onAChanged: console.log("in", index, a) }
    }
    Component { id: c; QtObject { property string t; property int a: root.n + root.o.v; onAChanged: console.log(t, a) } }
    property var made: []
    Timer {
        interval: 50; running: true
        onTriggered: {
            root.made.push(c.createObject(root, { t: "d1" }))
            root.made.push(c.createObject(root, { t: "d2" }))
            console.log("-- n")
            root.n = 2
            console.log("-- o.v")
            root.o.v = 2
            console.log("end")
            Qt.quit()
        }
    }
}
