// The delegates of several views over one list of objects, and a write to a property of a
// row object. Who reads the property of the object itself hears of it in the order the
// readers connected: a binding outside the delegates, a name of the row in the delegate's
// context, a required property named after it, `modelData.k`. Who reads it through the model
// item (`model.k`) hears after all of those: the item passes its object's signals on
// (QQmlDMObjectDataMetaObject, a QObject connection made when a property of the object is
// first read through the item, for all of them at once), and those come after the object's
// own readers, in the order the items connected.
// The handlers are silent while the delegates are made: this is about the write.
import QtQuick
import QtQml.Models
Item {
    id: root
    property bool live: false
    property list<QtObject> rows: [
        QtObject { property string nm: "x"; property int k: 1 },
        QtObject { property string nm: "y"; property int k: 2 }
    ]
    // The views are completed last one first, so the delegates below are made bottom up.
    // required properties named after the row's, in a Repeater and in an Instantiator
    Repeater {
        model: root.rows
        delegate: Item {
            required property int k; required property string nm; required property int index
            property string both: nm + k
            onNmChanged: if (root.live) console.log("rp nm", index, nm, k)
            onKChanged: if (root.live) console.log("rp k", index, nm, k)
            onBothChanged: if (root.live) console.log("rp both", index, both)
        }
    }
    Instantiator {
        model: root.rows
        delegate: QtObject {
            required property string nm; required property int k; required property int index
            property string both: nm + k
            onNmChanged: if (root.live) console.log("in nm", index, nm, k)
            onKChanged: if (root.live) console.log("in k", index, nm, k)
            onBothChanged: if (root.live) console.log("in both", index, both)
        }
    }
    // `model.k` of a required `model`, read when the delegate is made
    Repeater {
        model: root.rows
        delegate: Item {
            required property var model
            property int b: model.k
            onBChanged: if (root.live) console.log("rm model.k", model.index, b)
        }
    }
    // the names of the row in the delegate's context
    Repeater {
        model: root.rows
        delegate: Item {
            property string a: nm
            property int b: k
            onAChanged: if (root.live) console.log("cx a", index, a, b)
            onBChanged: if (root.live) console.log("cx b", index, a, b)
        }
    }
    // `model.k` of the context's `model`
    Instantiator {
        model: root.rows
        delegate: QtObject {
            property int b: model.k
            onBChanged: if (root.live) console.log("cm model.k", index, b)
        }
    }
    // a required `k` and `model.k` in one delegate
    Instantiator {
        model: root.rows
        delegate: QtObject {
            required property var model; required property int k
            property int b: model.k
            onKChanged: if (root.live) console.log("rk k", model.index, k)
            onBChanged: if (root.live) console.log("rk model.k", model.index, b)
        }
    }
    // `model.k` first read after every delegate is made
    Instantiator {
        id: late
        model: root.rows
        delegate: QtObject {
            required property var model
            property bool on: false
            property int b: on ? model.k : 0
            onBChanged: if (root.live) console.log("late model.k", model.index, b)
        }
    }
    // `k` of the context first read after every delegate is made
    Instantiator {
        id: lateName
        model: root.rows
        delegate: QtObject {
            property bool on: false
            property int b: on ? k : 0
            onBChanged: if (root.live) console.log("late k", index, b)
        }
    }
    // `model.nm` read when the delegate is made, `model.k` after every delegate is made:
    // the item passes on all the signals of its object from the first read
    Instantiator {
        id: lateOther
        model: root.rows
        delegate: QtObject {
            required property var model
            property bool on: false
            property string a: model.nm
            property int b: on ? model.k : 0
            onBChanged: if (root.live) console.log("nm then late model.k", model.index, b)
        }
    }
    // `model` read when the delegate is made, `model.k` after: reading the item connects
    // nothing
    Instantiator {
        id: lateItem
        model: root.rows
        delegate: QtObject {
            required property var model
            property bool on: false
            property var m: model
            property int b: on ? model.k : 0
            onBChanged: if (root.live) console.log("model then late model.k", model.index, b)
        }
    }
    // through modelData, and through model: made first
    Instantiator {
        model: root.rows
        delegate: QtObject {
            required property var modelData
            required property var model
            property string a: modelData.nm
            property int b: model.k
            onAChanged: if (root.live) console.log("md a", a, b)
            onBChanged: if (root.live) console.log("md b", a, b)
        }
    }
    // a binding outside any delegate, made before all of them
    property string outer: rows[1].nm + rows[0].k
    onOuterChanged: if (root.live) console.log("outer", outer)
    Timer {
        interval: 50; running: true
        onTriggered: {
            for (let i = 0; i < 2; i++) {
                lateItem.objectAt(i).on = true
                lateName.objectAt(i).on = true
                late.objectAt(i).on = true
                lateOther.objectAt(i).on = true
            }
            root.live = true
            console.log("-- nm")
            root.rows[1].nm = "z"
            console.log("-- k")
            root.rows[0].k = 7
            console.log("-- both of one row")
            root.rows[0].nm = "w"
            root.rows[0].k = 9
            console.log("-- the other row")
            root.rows[1].k = 5
            console.log("end")
            Qt.quit()
        }
    }
}
