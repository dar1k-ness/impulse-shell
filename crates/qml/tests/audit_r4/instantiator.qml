import QtQuick
import QtQml
Item {
    id: root
    property var m: ["a", "b"]
    Instantiator {
        id: inst
        model: root.m
        delegate: QtObject { required property var modelData; required property int index; objectName: modelData
            Component.onCompleted: console.log(" created", modelData, index, "count", inst.count, "objectAt", inst.objectAt(index) === this)
            Component.onDestruction: console.log(" destroyed", modelData) }
        onObjectAdded: (index, object) => console.log("objectAdded", index, object.objectName, "count", count)
        onObjectRemoved: (index, object) => console.log("objectRemoved", index, object.objectName, "count", count)
        onCountChanged: console.log("countChanged", count)
        onObjectChanged: console.log("objectChanged", object ? object.objectName : object)
        onActiveChanged: console.log("activeChanged", active)
    }
    Component.onCompleted: {
        console.log("A", inst.count, inst.object ? inst.object.objectName : inst.object, inst.asynchronous); m = ["a", "b", "c"];
        console.log("B", inst.count); inst.active = false;
        console.log("C", inst.count, inst.objectAt(0)); inst.active = true;
        console.log("D", inst.count, inst.objectAt(2).objectName, inst.objectAt(7)); inst.model = 1;
        console.log("E", inst.count);
        console.log("END"); Qt.quit();
    }
}
