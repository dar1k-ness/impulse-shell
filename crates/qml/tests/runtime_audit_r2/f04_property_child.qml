import QtQuick
// An Item made as the value of a property has no parent until `children: [iconItem]` runs:
// its own bindings are enabled first and read a null parent, then hear parentChanged.
Item {
    id: root; objectName: "root"; width: 100; height: 40
    property bool on: true
    Timer { interval: 50; running: true; onTriggered: { root.on = false; console.log("off") } }
    Timer { interval: 100; running: true; onTriggered: { root.on = true; console.log("on") } }
    Row {
        id: row
        Area {
            id: a1
            iconItem: Icon {
                id: ic; objectName: "ic"
                anchors.verticalCenter: { console.log("vc eval", parent ? parent.objectName : parent); return parent.verticalCenter }
                icon: "wifi"
                opacity: { console.log("ic opacity eval"); return 1 }
                onParentChanged: console.log("ic parent", parent ? parent.objectName : parent)
            }
        }
    }
    Loader {
        active: root.on
        sourceComponent: Area {
            objectName: "loaded"
            iconItem: Icon {
                objectName: "ic2"
                anchors.verticalCenter: parent.verticalCenter
                icon: "b"
                onParentChanged: console.log("ic2 parent", parent ? parent.objectName : parent)
                Component.onDestruction: console.log("ic2 gone")
            }
            Component.onCompleted: console.log("loaded", children.length, children[0].objectName, iconItem.parent.objectName)
        }
    }
    component Icon: Item {
        required property string icon
        implicitWidth: 20
        implicitHeight: 20
    }
    component Area: MouseArea {
        id: area; objectName: "area"
        required property var iconItem
        anchors { top: { console.log("area top eval"); return parent ? parent.top : undefined } }
        implicitHeight: { console.log("area ih eval"); return area.iconItem.implicitHeight }
        children: { console.log("children eval"); return [iconItem] }
        opacity: { console.log("area opacity eval"); return 1 }
    }
    Component.onCompleted: console.log("done", ic.parent.objectName, a1.children.length, a1.implicitHeight)
}
