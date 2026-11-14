import QtQuick
Item {
    id: root; width: 401; height: 301
    Rectangle { objectName: "hcAndLeft"; anchors.left: parent.left; anchors.horizontalCenter: parent.horizontalCenter; width: 20; height: 5 }
    Rectangle { objectName: "hcAndRight"; y: 10; anchors.right: parent.right; anchors.rightMargin: 30; anchors.horizontalCenter: parent.horizontalCenter; anchors.horizontalCenterOffset: 10; width: 20; height: 5 }
    Rectangle { objectName: "vcAndTop"; x: 10; anchors.top: parent.top; anchors.topMargin: 20; anchors.verticalCenter: parent.verticalCenter; width: 5; height: 20 }
    Rectangle { objectName: "vcAndBottom"; x: 20; anchors.bottom: parent.bottom; anchors.verticalCenter: parent.verticalCenter; anchors.verticalCenterOffset: -40; width: 5; height: 20 }
    Item { id: box; objectName: "box"; x: 50; y: 60; width: 120; height: 80
        Rectangle { objectName: "hcLeftSibling"; anchors.left: inner.right; anchors.leftMargin: 4; anchors.horizontalCenter: parent.horizontalCenter; height: 5 }
        Rectangle { id: inner; objectName: "inner"; width: 20; height: 20 }
    }
    Rectangle { objectName: "hc"; anchors.horizontalCenter: parent.horizontalCenter; anchors.horizontalCenterOffset: 2.5; y: 250; width: 33; height: 11 }
    Rectangle { objectName: "leftAndWidth"; anchors.left: parent.left; anchors.leftMargin: 11; y: 270; width: 20; height: 5 }
}
