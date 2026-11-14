// Functions connected to a row object's own signal and the model items of the row (each
// connects when a property is first read through it, or when a Connections looks a handler
// up on it) are called in the order they connected, after the object's own readers and
// Connections; a disconnected function is not called.
// qt: outer 7;row conn;D conn 7;f0;A model.k 7;f1;C model.k 7;f2;B model.k 7;f3;--;outer 8;row conn;D conn 8;f0;A model.k 8;f1;C model.k 8;B model.k 8;f3;--;C model.nm 4
import QtQuick
import QtQml.Models
Item {
    id: root
    property string result: ""
    property var seen: []
    property bool live: false
    property list<QtObject> rows: [
        QtObject { property string nm: "x"; property int k: 1 }
    ]
    property int outer: rows[0].k
    onOuterChanged: if (live) seen.push("outer " + outer)
    Connections {
        target: root.rows[0]
        function onKChanged() { root.seen.push("row conn") }
    }
    Repeater {
        id: a
        model: root.rows
        delegate: Item {
            id: ad
            required property var model
            property bool on: false
            property int b: on ? model.k : -1
            onBChanged: if (root.live) root.seen.push("A model.k " + b)
        }
    }
    Repeater {
        id: b
        model: root.rows
        delegate: Item {
            id: bd
            required property var model
            property bool on: false
            property int b: on ? model.k : -1
            onBChanged: if (root.live) root.seen.push("B model.k " + b)
        }
    }
    Repeater {
        model: root.rows
        delegate: Item {
            id: dd
            required property var model
            Connections {
                target: dd.model
                function onKChanged() { root.seen.push("D conn " + dd.model.k) }
            }
        }
    }
    Instantiator {
        id: c
        model: root.rows
        delegate: QtObject {
            required property var model
            property bool on: false
            property int b: on ? model.nm.length : -1
            onBChanged: if (root.live) root.seen.push("C model.nm " + b)
            property int d: on ? model.k : -1
            onDChanged: if (root.live) root.seen.push("C model.k " + d)
        }
    }
    function f2() { seen.push("f2") }
    Timer {
        interval: 0; running: true
        onTriggered: {
            const o = root.rows[0]
            o.kChanged.connect(() => root.seen.push("f0"))
            a.itemAt(0).on = true
            o.kChanged.connect(() => root.seen.push("f1"))
            c.objectAt(0).on = true
            o.kChanged.connect(root.f2)
            b.itemAt(0).on = true
            o.kChanged.connect(() => root.seen.push("f3"))
            root.live = true
            o.k = 7
            root.seen.push("--")
            o.kChanged.disconnect(root.f2)
            o.k = 8
            root.seen.push("--")
            o.nm = "long"
            root.result = root.seen.join(";")
        }
    }
}
