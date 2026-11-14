// A write to a property of a row object of a list of objects: the readers of the object's
// own property hear of it in the order they connected (a binding outside the delegates, a
// name of the row in the context, a required property, `modelData.k`), the readers of
// `model.k` after all of them, in the order their model items connected to the object: at the
// first read of one of its properties through the item, not when the item is made.
// qt: outer 9;modelData.k 0 7;ctx 0 7;req 0 7;late ctx 0 7;md model.k 0 7;nm, late model.k 0 7;ctx model.k 0 7;model.k 0 7;late model.k 0 7;--;outer 12;modelData.k 1 5;ctx 1 5;req 1 5;late ctx 1 5;md model.k 1 5;nm, late model.k 1 5;ctx model.k 1 5;model.k 1 5;late model.k 1 5
import QtQuick
import QtQml.Models
Item {
    id: root
    property string result: ""
    property var seen: []
    property bool live: false
    property list<QtObject> rows: [
        QtObject { property string nm: "x"; property int k: 1 },
        QtObject { property string nm: "y"; property int k: 2 }
    ]
    Repeater {
        model: root.rows
        delegate: Item {
            required property int k; required property int index
            onKChanged: if (root.live) root.seen.push("req " + index + " " + k)
        }
    }
    Repeater {
        model: root.rows
        delegate: Item {
            required property var model
            property int b: model.k
            onBChanged: if (root.live) root.seen.push("model.k " + model.index + " " + b)
        }
    }
    Repeater {
        model: root.rows
        delegate: Item {
            property int b: k
            onBChanged: if (root.live) root.seen.push("ctx " + index + " " + b)
        }
    }
    Instantiator {
        model: root.rows
        delegate: QtObject {
            property int b: model.k
            onBChanged: if (root.live) root.seen.push("ctx model.k " + index + " " + b)
        }
    }
    Instantiator {
        id: late
        model: root.rows
        delegate: QtObject {
            required property var model
            property bool on: false
            property int b: on ? model.k : 0
            onBChanged: if (root.live) root.seen.push("late model.k " + model.index + " " + b)
        }
    }
    Instantiator {
        id: lateName
        model: root.rows
        delegate: QtObject {
            property bool on: false
            property int b: on ? k : 0
            onBChanged: if (root.live) root.seen.push("late ctx " + index + " " + b)
        }
    }
    Instantiator {
        id: lateOther
        model: root.rows
        delegate: QtObject {
            required property var model
            property bool on: false
            property string a: model.nm
            property int b: on ? model.k : 0
            onBChanged: if (root.live) root.seen.push("nm, late model.k " + model.index + " " + b)
        }
    }
    Instantiator {
        model: root.rows
        delegate: QtObject {
            required property var modelData; required property var model
            property int a: modelData.k
            property int b: model.k
            onAChanged: if (root.live) root.seen.push("modelData.k " + model.index + " " + a)
            onBChanged: if (root.live) root.seen.push("md model.k " + model.index + " " + b)
        }
    }
    property int outer: rows[0].k + rows[1].k
    onOuterChanged: if (root.live) seen.push("outer " + outer)
    Timer {
        interval: 0; running: true
        onTriggered: {
            for (let i = 0; i < 2; i++) {
                lateName.objectAt(i).on = true
                late.objectAt(i).on = true
                lateOther.objectAt(i).on = true
            }
            root.live = true
            root.rows[0].k = 7
            root.seen.push("--")
            root.rows[1].k = 5
            root.result = root.seen.join(";")
        }
    }
}
