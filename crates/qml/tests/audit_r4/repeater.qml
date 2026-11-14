import QtQuick
Item {
    id: root
    property var m: ["a", "b"]
    Repeater {
        id: rep
        model: root.m
        delegate: Item { required property var modelData; required property int index; objectName: modelData
            Component.onCompleted: console.log(" created", modelData, index, "count", rep.count, "itemAt", rep.itemAt(index) === this)
            Component.onDestruction: console.log(" destroyed", modelData) }
        onItemAdded: (index, item) => console.log("itemAdded", index, item.objectName, "count", count)
        onItemRemoved: (index, item) => console.log("itemRemoved", index, item.objectName, "count", count)
        onCountChanged: console.log("countChanged", count)
        onModelChanged: console.log("modelChanged")
    }
    Component.onCompleted: {
        console.log("A"); m = ["a", "b", "c"];
        console.log("B", rep.count, root.children.length); m = ["c"];
        console.log("C", rep.count, rep.itemAt(0).objectName, rep.itemAt(5), rep.itemAt(-1)); rep.model = 2;
        console.log("D", rep.count); rep.model = 0;
        console.log("E", rep.count); rep.model = -3;
        console.log("F", rep.count); rep.delegate = null; rep.model = 2
        console.log("G", rep.count, root.children.length);
        console.log("END"); Qt.quit();
    }
}
