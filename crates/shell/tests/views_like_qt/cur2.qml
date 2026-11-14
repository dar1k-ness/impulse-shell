import QtQuick
Item {
    id: root
    width: 200; height: 400
    property var arr: ["p", "q"]
    component D: Rectangle { required property int index; width: 50; height: 20; objectName: "d" + index; Component.onCompleted: console.log("made", index) }
    ListView {
        id: a; width: 50; height: 100; model: root.arr; delegate: D {}
        onCountChanged: console.log("a count", count, currentIndex)
        onCurrentIndexChanged: console.log("a cur", currentIndex, count)
    }
    ListView {
        id: c; width: 50; height: 100; model: 3; delegate: D {}
        onCurrentIndexChanged: console.log("c cur", currentIndex, count, currentItem ? currentItem.objectName : null)
        onCurrentItemChanged: console.log("c item", currentIndex, currentItem ? currentItem.objectName : null)
        onCountChanged: console.log("c count", count, currentIndex)
    }
    Repeater {
        model: root.arr
        Item { required property int index; Component.onCompleted: console.log("rep", index) }
    }
    Component.onCompleted: console.log("completed")
    Timer {
        interval: 100; running: true
        onTriggered: {
            console.log("-- arr"); root.arr = ["z"]
            console.log("end")
        }
    }
}
