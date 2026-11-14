// Rows that are objects with bound properties, in a Repeater and a ListView: what the
// bindings that read through the model items, the Connections on the items and a function
// connected to the object's own signal hear while the delegates are made and after, and of a
// write a Connections handler makes to another property of its row.
import QtQuick
import QtQml.Models
Item {
    id: root
    property bool live: false
    property int src: 1
    property list<QtObject> rows: [
        QtObject { property string nm: "x"; property int k: root.src * 10; property int j: 0 },
        QtObject { property string nm: "y"; property int k: root.src * 20; property int j: 0 }
    ]
    Repeater {
        id: a
        model: root.rows
        delegate: Item {
            id: d
            required property var model
            required property int index
            property int b: model.k
            onBChanged: console.log("A binding " + index + " " + b)
            Connections {
                target: d.model
                function onKChanged() { console.log("A conn " + d.index + " " + d.model.k); d.model.j = d.model.k }
                function onJChanged() { console.log("A conn j " + d.index + " " + d.model.j) }
            }
            Component.onCompleted: {
                console.log("completed " + index + " " + b)
                modelData.kChanged.connect(() => console.log("f " + index + " " + modelData.k))
            }
            required property var modelData
        }
    }
    ListView {
        id: lv
        width: 100; height: 100
        model: root.rows
        delegate: Item {
            id: v
            width: 100; height: 20
            required property var model
            required property int index
            property int b: model.j + model.k
            onBChanged: console.log("V binding " + index + " " + b)
        }
    }
    Component.onCompleted: { console.log("root completed"); src = 2 }
    Timer {
        interval: 0; running: true
        onTriggered: {
            console.log("--")
            root.src = 3
            console.log("--")
            root.rows[1].k = 1
            Qt.quit()
        }
    }
}
