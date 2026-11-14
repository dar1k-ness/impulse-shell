import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
// The two shapes the shell's own QML puts a layered item in. The lock screen's password field
// (qml/modules/rusty/lock/LockSurface.qml in a Toolbar.qml): a layered TextField between a Loader
// that is off and a button, in a RowLayout that sizes its background. The cheatsheet
// (qml/modules/rusty/cheatsheet/Cheatsheet.qml): a layered SwipeView that fills a ColumnLayout
// under a toolbar.
Item {
    id: root; width: 800; height: 600
    property Item fieldFx: null
    property Item swipeFx: null
    Rectangle {
        objectName: "bar"
        implicitHeight: 56; implicitWidth: row.implicitWidth + 16
        width: implicitWidth; height: implicitHeight
        RowLayout {
            id: row; objectName: "row"; spacing: 4
            anchors { fill: parent; margins: 8 }
            Loader {
                objectName: "finger"
                Layout.leftMargin: 10; Layout.alignment: Qt.AlignVCenter
                active: false; visible: active
                sourceComponent: Rectangle { implicitWidth: 20; implicitHeight: 20 }
            }
            TextField {
                objectName: "field"
                Layout.fillHeight: true; Layout.rightMargin: -Layout.leftMargin
                implicitWidth: 200; implicitHeight: 34; padding: 10; clip: true
                layer.enabled: true
                layer.effect: ShaderEffect { Component.onCompleted: root.fieldFx = this }
            }
            Rectangle { objectName: "ok"; implicitWidth: 40; implicitHeight: 40 }
        }
    }
    Rectangle {
        objectName: "sheet"; y: 100
        implicitWidth: col.implicitWidth + 60; implicitHeight: col.implicitHeight + 60
        width: implicitWidth; height: implicitHeight
        ColumnLayout {
            id: col; objectName: "col"; spacing: 10
            anchors.centerIn: parent
            Rectangle { objectName: "tabs"; Layout.alignment: Qt.AlignHCenter; implicitWidth: 120; implicitHeight: 56 }
            SwipeView {
                id: swipe; objectName: "swipe"
                Layout.topMargin: 5; Layout.fillWidth: true; Layout.fillHeight: true
                spacing: 10
                implicitWidth: Math.max.apply(null, contentChildren.map(child => child.implicitWidth || 0))
                implicitHeight: Math.max.apply(null, contentChildren.map(child => child.implicitHeight || 0))
                clip: true
                layer.enabled: true
                layer.effect: ShaderEffect { Component.onCompleted: root.swipeFx = this }
                Item { implicitWidth: 300; implicitHeight: 200 }
                Item { implicitWidth: 250; implicitHeight: 220 }
            }
        }
    }
    // The effect items are where their items are, and the SwipeView counts its two pages only.
    Item {
        objectName: "field_fx_at"
        x: root.fieldFx ? root.fieldFx.x : -1; y: root.fieldFx ? root.fieldFx.y : -1
        width: root.fieldFx ? root.fieldFx.width : -1; height: root.fieldFx ? root.fieldFx.height : -1
    }
    Item {
        objectName: "swipe_fx_at"
        x: root.swipeFx ? root.swipeFx.x : -1; y: root.swipeFx ? root.swipeFx.y : -1
        width: root.swipeFx ? root.swipeFx.width : -1; height: root.swipeFx ? root.swipeFx.height : -1
    }
    Item { objectName: "counts"; x: row.children.length; y: col.children.length; width: swipe.count; height: swipe.currentIndex }
}
