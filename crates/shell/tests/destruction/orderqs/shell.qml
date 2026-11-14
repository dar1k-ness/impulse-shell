import QtQuick
import Quickshell
Scope {
    id: root
    component D: QtObject {
        property string n
        Component.onDestruction: console.log("destroyed", n)
    }
    property var free: null
    property var owned: null
    Component { id: dyn; D {} }
    Component.onCompleted: {
        free = dyn.createObject(null, { n: "created without a parent" })
        owned = dyn.createObject(root, { n: "created under root" })
        console.log("ready", Single.name)
    }
    Component.onDestruction: console.log("destroyed root, sees", first.n, parent1.objectName)
    D { id: first; n: "first object of root" }
    Item {
        id: parent1
        objectName: "parent"
        Component.onDestruction: console.log("destroyed parent")
        Item {
            Component.onDestruction: console.log("destroyed child 1")
            Item { Component.onDestruction: console.log("destroyed grandchild") }
        }
        D { n: "object of parent" }
        Item { Component.onDestruction: console.log("destroyed child 2") }
        Loader {
            Component.onDestruction: console.log("destroyed loader")
            sourceComponent: Item {
                Component.onDestruction: console.log("destroyed loader item")
                Item { Component.onDestruction: console.log("destroyed loader item child") }
            }
        }
        Loader { active: false; sourceComponent: D { n: "never loaded" } }
        Repeater {
            model: 3
            Component.onDestruction: console.log("destroyed repeater")
            Item {
                required property int index
                Component.onDestruction: console.log("destroyed delegate", index)
                D { n: "object of delegate " + parent.index }
            }
        }
    }
    LazyLoader {
        active: true
        Component.onDestruction: console.log("destroyed lazy loader")
        Scope {
            Component.onDestruction: console.log("destroyed lazy loader item")
            D { n: "object of lazy loader item" }
        }
    }
    LazyLoader { D { n: "never loaded lazily" } }
    Variants {
        model: ["x", "y"]
        Component.onDestruction: console.log("destroyed variants")
        Scope {
            required property string modelData
            Component.onDestruction: console.log("destroyed variant", modelData)
        }
    }
    D { n: "last object of root" }
    Timer { interval: 60; running: true; onTriggered: { console.log("quit"); Qt.quit() } }
}
