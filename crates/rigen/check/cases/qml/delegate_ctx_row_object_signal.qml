// The model item of a row that is an object has the properties of the object and none of
// its signals, read or not: `model.kChanged` is undefined for a script and
// `model.kChanged.connect` throws a TypeError. What reads `model.k` hears of a write as before.
// qt: unread 0 is undefined;unread 0: TypeError;then 1 is undefined undefined;then 0: TypeError;then nm 0: TypeError;read 0: TypeError;ctx 0: TypeError;unread 1 is undefined;unread 1: TypeError;then 2 is undefined undefined;then 1: TypeError;then nm 1: TypeError;read 1: TypeError;ctx 1: TypeError;--;outer 9;after binding 0 7;read binding 0 7;--;--;outer 12;after binding 1 5;read binding 1 5
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
    property int outer: rows[0].k + rows[1].k
    onOuterChanged: if (live) seen.push("outer " + outer)
    function hear(what, sig) {
        try {
            sig().connect(() => root.seen.push(what))
            root.seen.push(what + " connected")
        } catch (e) {
            root.seen.push(what + ": " + (e instanceof TypeError ? "TypeError" : e))
        }
    }
    Instantiator {
        id: unread
        model: root.rows
        delegate: QtObject {
            required property var model
            function hook() {
                root.seen.push("unread " + model.index + " is " + typeof model.kChanged)
                root.hear("unread " + model.index, () => model.kChanged)
                root.seen.push("then " + model.k + " is " + typeof model.kChanged + " " + typeof model.nmChanged)
                root.hear("then " + model.index, () => model.kChanged)
                root.hear("then nm " + model.index, () => model.nmChanged)
            }
        }
    }
    Repeater {
        id: read
        model: root.rows
        delegate: Item {
            required property var model
            property int b: model.k
            onBChanged: if (root.live) root.seen.push("read binding " + model.index + " " + b)
            function hook() { root.hear("read " + model.index, () => model.kChanged) }
        }
    }
    Repeater {
        id: ctx
        model: root.rows
        delegate: Item {
            function hook() {
                root.hear("ctx " + index, () => model.kChanged)
            }
        }
    }
    Instantiator {
        id: after
        model: root.rows
        delegate: QtObject {
            required property var model
            property int b: model.k
            onBChanged: if (root.live) root.seen.push("after binding " + model.index + " " + b)
        }
    }
    Timer {
        interval: 0; running: true
        onTriggered: {
            for (let i = 0; i < 2; i++) {
                unread.objectAt(i).hook()
                read.itemAt(i).hook()
                ctx.itemAt(i).hook()
            }
            root.live = true
            root.seen.push("--")
            root.rows[0].k = 7
            root.seen.push("--")
            root.rows[1].nm = "z"
            root.seen.push("--")
            root.rows[1].k = 5
            root.result = root.seen.join(";")
        }
    }
}
