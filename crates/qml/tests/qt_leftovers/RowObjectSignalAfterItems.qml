// A function connected to the signal of a row object itself (`modelData.kChanged.connect`)
// after the model items of the row connected to it: it is called after the readers of
// `model.k`, a QObject connection like theirs, in the order the connections were made.
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
    Repeater {
        id: read
        model: root.rows
        delegate: Item {
            required property var model
            required property var modelData
            property int b: model.k
            onBChanged: if (root.live) console.log("model.k " + model.index + " " + b)
            function hook() {
                modelData.kChanged.connect(() => console.log("connected " + model.index))
            }
        }
    }
    Instantiator {
        model: root.rows
        delegate: QtObject {
            required property var model
            property int b: model.k
            onBChanged: if (root.live) console.log("other model.k " + model.index + " " + b)
        }
    }
    Timer {
        interval: 0; running: true
        onTriggered: {
            read.itemAt(0).hook()
            read.itemAt(1).hook()
            root.live = true
            root.rows[0].k = 7
            console.log("--")
            root.rows[1].k = 5
            Qt.quit()
        }
    }
}
