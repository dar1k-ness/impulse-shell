import QtQuick
import Qt.labs.qmlmodels
Item {
    id: root
    property var m: [ { kind: "a", v: 1 }, { kind: "b", v: 2 }, { kind: "zzz", v: 3 }, { kind: "a", v: 4 } ]
    Repeater {
        id: rep
        model: root.m
        delegate: DelegateChooser {
            id: ch
            role: "kind"
            DelegateChoice { roleValue: "a"; Item { required property var modelData; required property int index; Component.onCompleted: console.log("A delegate", index, modelData.v) } }
            DelegateChoice { index: 1; Item { required property int index; Component.onCompleted: console.log("index-1 delegate", index) } }
            DelegateChoice { roleValue: "b"; Item { required property int index; Component.onCompleted: console.log("B delegate", index) } }
            DelegateChoice { Item { required property int index; Component.onCompleted: console.log("default delegate", index) } }
        }
    }
    Component.onCompleted: { console.log("count", rep.count, root.children.length); m = [ { kind: "b", v: 9 } ]; console.log("count", rep.count); console.log("END"); Qt.quit() }
}
