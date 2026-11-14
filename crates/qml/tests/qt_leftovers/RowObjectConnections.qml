// A Connections whose target is the model item of a row that is an object: `onKChanged` is
// a handler of the item's signal, which the item passes on from its object's after the
// object's own readers and handlers. Looking the handler up reads the property through the
// item, so the item connects to its object then, read or not.
import QtQuick
import QtQml.Models
Item {
    id: root
    property bool live: false
    property list<QtObject> rows: [
        QtObject { property string nm: "x"; property int k: 1 },
        QtObject { property string nm: "y"; property int k: 2 }
    ]
    property int outer: rows[0].k + rows[1].k
    onOuterChanged: if (live) console.log("outer " + outer)
    Connections {
        target: root.rows[0]
        function onKChanged() { console.log("row conn " + root.rows[0].k) }
    }
    Instantiator {
        model: root.rows
        delegate: QtObject {
            id: unread
            required property var model
            property Connections c: Connections {
                target: unread.model
                function onKChanged() { console.log("unread conn " + unread.model.index + " " + unread.model.k) }
                function onNmChanged() { console.log("unread conn nm " + unread.model.index + " " + unread.model.nm) }
            }
        }
    }
    Repeater {
        model: root.rows
        delegate: Item {
            id: read
            required property var model
            property int b: model.k
            onBChanged: if (root.live) console.log("read binding " + model.index + " " + b)
            Connections {
                target: read.model
                function onKChanged() { console.log("read conn " + read.model.index + " " + read.model.k) }
                function onNmChanged() { console.log("read conn nm " + read.model.index + " " + read.model.nm) }
            }
        }
    }
    Repeater {
        model: root.rows
        delegate: Item {
            Connections {
                target: model
                function onKChanged() { console.log("ctx conn " + index + " " + model.k) }
            }
            Connections {
                target: modelData
                function onKChanged() { console.log("modelData conn " + index + " " + modelData.k) }
            }
        }
    }
    Instantiator {
        model: root.rows
        delegate: QtObject {
            id: after
            required property var model
            property int b: model.k
            onBChanged: if (root.live) console.log("after binding " + model.index + " " + b)
            property Connections c: Connections {
                target: after.model
                function onKChanged() { console.log("after conn " + after.model.index + " " + after.model.k) }
            }
        }
    }
    Timer {
        interval: 0; running: true
        onTriggered: {
            root.live = true
            root.rows[0].k = 7
            console.log("--")
            root.rows[1].nm = "z"
            console.log("--")
            root.rows[1].k = 5
            Qt.quit()
        }
    }
}
