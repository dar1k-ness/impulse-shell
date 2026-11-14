// Component.onCompleted of every object of the tree, of both document layers of one object
// and of an object bound to a property.
// qt: root base.done c3 c2 c1 gc p
import QtQuick
Item {
    property string result: Log.s
    Component.onCompleted: Log.add("root")
    Item {
        Component.onCompleted: Log.add("c1")
        Item { Component.onCompleted: Log.add("gc") }
    }
    property Item p: Item { Component.onCompleted: Log.add("p") }
    Item { Component.onCompleted: Log.add("c2") }
    Base { Component.onCompleted: Log.add("c3") }
}
