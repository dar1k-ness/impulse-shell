// QQmlObjectCreator::finalize enables the bindings of the whole tree last created first.
// qt: c2.u gc.w c1.u c1.v root.r1 root.r2
import QtQuick
Item {
    id: root
    property string result: Log.s
    property int k: 1
    property int r1: k + 1
    onR1Changed: Log.add("root.r1")
    Item {
        property int u: root.k + 1
        onUChanged: Log.add("c1.u")
        property int v: root.k + 2
        onVChanged: Log.add("c1.v")
        Item {
            property int w: root.k + 3
            onWChanged: Log.add("gc.w")
        }
    }
    Item {
        property int u: root.k + 1
        onUChanged: Log.add("c2.u")
    }
    property int r2: k + 2
    onR2Changed: Log.add("root.r2")
}
