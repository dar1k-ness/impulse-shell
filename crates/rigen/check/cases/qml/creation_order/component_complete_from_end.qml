// QQmlObjectCreator::finalize calls componentComplete last created first: the Repeater and
// the Loader created last make their items first; Component.onCompleted follows all of them.
// qt: r3 l2 r1 root
import QtQuick
Item {
    property string result: Log.s
    Component.onCompleted: Log.add("root")
    Repeater {
        model: 1
        Item { Component.onCompleted: Log.add("r1") }
    }
    Item {
        Loader {
            sourceComponent: Item { Component.onCompleted: Log.add("l2") }
        }
    }
    Repeater {
        model: 1
        Item { Component.onCompleted: Log.add("r3") }
    }
}
