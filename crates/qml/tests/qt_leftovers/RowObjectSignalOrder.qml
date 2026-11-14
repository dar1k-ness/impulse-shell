// Functions connected to the signal of a row object itself and the model items of the row
// (each connects when a property is first read through it) are QObject connections of that
// signal: they are called in the order they were made, after the object's own readers and
// Connections.
import QtQuick
import QtQml.Models
Item {
    id: root
    property bool live: false
    property list<QtObject> rows: [
        QtObject { property string nm: "x"; property int k: 1 }
    ]
    property int outer: rows[0].k
    onOuterChanged: if (live) console.log("outer " + outer)
    Connections {
        target: root.rows[0]
        function onKChanged() { console.log("row conn") }
    }
    Repeater {
        id: a
        model: root.rows
        delegate: Item {
            required property var model
            property bool on: false
            property int b: on ? model.k : -1
            onBChanged: if (root.live) console.log("A model.k " + b)
        }
    }
    Repeater {
        id: b
        model: root.rows
        delegate: Item {
            required property var model
            property bool on: false
            property int b: on ? model.k : -1
            onBChanged: if (root.live) console.log("B model.k " + b)
        }
    }
    Instantiator {
        id: c
        model: root.rows
        delegate: QtObject {
            required property var model
            property bool on: false
            property int b: on ? model.nm.length : -1
            onBChanged: if (root.live) console.log("C model.nm " + b)
            property int d: on ? model.k : -1
            onDChanged: if (root.live) console.log("C model.k " + d)
        }
    }
    function f2() { console.log("f2") }
    Timer {
        interval: 0; running: true
        onTriggered: {
            const o = root.rows[0]
            o.kChanged.connect(() => console.log("f0"))
            a.itemAt(0).on = true
            o.kChanged.connect(() => console.log("f1"))
            c.objectAt(0).on = true
            o.kChanged.connect(root.f2)
            b.itemAt(0).on = true
            o.kChanged.connect(() => { console.log("f3"); })
            root.live = true
            o.k = 7
            console.log("--")
            o.kChanged.disconnect(root.f2)
            o.k = 8
            console.log("--")
            o.nm = "long"
            Qt.quit()
        }
    }
}
