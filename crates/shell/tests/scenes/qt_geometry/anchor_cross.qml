import QtQuick
Item {
    id: root; width: 401; height: 301
    Rectangle { objectName: "lrCross"; anchors.left: parent.right; anchors.right: parent.left; height: 5 }
    Rectangle { objectName: "tbCross"; anchors.top: parent.bottom; anchors.bottom: parent.top; width: 5 }
    Rectangle { objectName: "lcCross"; y: 20; anchors.left: parent.right; anchors.horizontalCenter: parent.horizontalCenter; height: 5 }
    Item { id: box; objectName: "box"; x: 50; y: 60; width: 20; height: 10
        Rectangle { objectName: "lrMargins"; anchors.left: parent.left; anchors.right: parent.right; anchors.leftMargin: 15; anchors.rightMargin: 12; height: 5 }
        Rectangle { objectName: "tbMargins"; anchors.top: parent.top; anchors.bottom: parent.bottom; anchors.topMargin: 8; anchors.bottomMargin: 9; width: 5 }
        Rectangle { objectName: "fillMargins"; anchors.fill: parent; anchors.margins: 12 }
        Item { objectName: "crossed"; anchors.left: parent.right; anchors.right: parent.left; height: 10
            Rectangle { objectName: "fillCrossed"; anchors.fill: parent }
            Rectangle { objectName: "centerInCrossed"; anchors.centerIn: parent; width: 4; height: 4 }
            Rectangle { objectName: "rightOfCrossed"; anchors.left: parent.right; width: 4; height: 4 }
        }
    }
}
