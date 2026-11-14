import QtQuick
import QtQml.Models
Item {
    id: root
    Item {
        id: a
        objectName: { console.log("a.objectName"); return "A" }
        x: { console.log("a.x"); return 7 }
        width: { console.log("a.width"); return 30 }
    }
    ItemSelectionModel { id: sel; model: { console.log("sel.model"); return lm } }
    ListModel { id: lm }
    property string seenName: { console.log("seenName [" + a.objectName + "]"); return a.objectName }
    property real seenX: { console.log("seenX " + a.x); return a.x }
    property real seenWidth: { console.log("seenWidth " + a.width); return a.width }
    property bool seenModel: { console.log("seenModel " + (sel.model === lm) + " " + (sel.model === null) + " " + typeof sel.model); return sel.model === lm }
    Component.onCompleted: { console.log("completed", seenName, seenX, seenWidth, seenModel); Qt.quit() }
}
