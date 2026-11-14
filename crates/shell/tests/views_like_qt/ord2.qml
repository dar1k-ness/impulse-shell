import QtQuick
import QtQml.Models
Item {
    id: root
    width: 200; height: 200
    property list<QtObject> rows: [
        QtObject { property string nm: "x"; property int k: 1 },
        QtObject { property string nm: "y"; property int k: 2 }
    ]
    property list<QtObject> more: [
        QtObject { property string nm: "p"; property int k: 5 },
        QtObject { property string nm: "q"; property int k: 6 }
    ]
    Repeater {
        model: root.rows
        delegate: Item {
            required property int k; required property var model; required property string nm; required property int index; required property var modelData
            onModelDataChanged: console.log("rp data", index, nm, k, typeof modelData, typeof model)
            onIndexChanged: console.log("rp idx", index, nm, k, typeof modelData, typeof model)
            onNmChanged: console.log("rp nm", index, nm, k, typeof modelData, typeof model)
            onKChanged: console.log("rp k", index, nm, k, typeof modelData, typeof model)
            onModelChanged: console.log("rp model", index, nm, k, typeof modelData, typeof model)
            Component.onCompleted: console.log("rp +", index, nm, k, modelData === root.rows[index])
        }
    }
    Instantiator {
        model: root.more
        delegate: QtObject {
            required property string nm; required property var modelData; required property int k; required property int index
            onModelDataChanged: console.log("in data", index, nm, k, typeof modelData)
            onIndexChanged: console.log("in idx", index, nm, k, typeof modelData)
            onNmChanged: console.log("in nm", index, nm, k, typeof modelData)
            onKChanged: console.log("in k", index, nm, k, typeof modelData)
            Component.onCompleted: console.log("in +", index, nm, k, modelData === root.more[index])
        }
    }
    Timer { interval: 100; running: true; onTriggered: { console.log("-- write"); root.rows[1].nm = "z"; root.more[0].k = 7; root.rows[0].k = 8; root.more[1].nm = "r"; console.log("end") } }
}
