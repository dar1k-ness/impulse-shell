import QtQuick
import QtQuick.Layouts
Item {
    id: root; width: 400; height: 300
    RowLayout { objectName: "rl1"; x: 0; y: 0; width: 300; height: 40; spacing: 6
        Rectangle { objectName: "rl1a"; implicitWidth: 30; implicitHeight: 10 }
        Rectangle { objectName: "rl1b"; Layout.fillWidth: true; implicitHeight: 20 }
        Rectangle { objectName: "rl1c"; Layout.fillWidth: true; Layout.preferredWidth: 100; Layout.maximumWidth: 110; Layout.fillHeight: true }
        Rectangle { objectName: "rl1d"; width: 20; height: 50 }
        Rectangle { objectName: "rl1e"; visible: false; implicitWidth: 500; implicitHeight: 500 }
    }
    RowLayout { objectName: "rl2"; x: 0; y: 50
        Rectangle { objectName: "rl2a"; implicitWidth: 31; implicitHeight: 13 }
        Rectangle { objectName: "rl2b"; Layout.preferredWidth: 20; Layout.preferredHeight: 7; Layout.alignment: Qt.AlignBottom; Layout.leftMargin: 3; Layout.rightMargin: 4; Layout.topMargin: 5 }
        Rectangle { objectName: "rl2c"; Layout.minimumWidth: 40; Layout.fillWidth: true; implicitWidth: 10; implicitHeight: 5; Layout.margins: 2 }
        Item { objectName: "rl2d" }
        Item { objectName: "rl2e"; Layout.fillWidth: true; Layout.fillHeight: true }
    }
    RowLayout { objectName: "rl3"; x: 0; y: 100; width: 100; height: 30; spacing: 0
        Rectangle { objectName: "rl3a"; Layout.fillWidth: true; implicitWidth: 10; implicitHeight: 10 }
        Rectangle { objectName: "rl3b"; Layout.fillWidth: true; implicitWidth: 10; implicitHeight: 10; Layout.alignment: Qt.AlignRight | Qt.AlignTop }
        Rectangle { objectName: "rl3c"; Layout.fillWidth: true; implicitWidth: 10; implicitHeight: 10; Layout.alignment: Qt.AlignHCenter; Layout.maximumWidth: 20 }
    }
    RowLayout { objectName: "rl4"; x: 0; y: 140; width: 50; height: 20; spacing: 5
        Rectangle { objectName: "rl4a"; implicitWidth: 40; implicitHeight: 10 }
        Rectangle { objectName: "rl4b"; implicitWidth: 40; implicitHeight: 10; Layout.minimumWidth: 30 }
        Rectangle { objectName: "rl4c"; Layout.preferredWidth: 40; Layout.fillWidth: true; implicitHeight: 10 }
    }
    RowLayout { objectName: "rl5"; x: 0; y: 170; width: 200; uniformCellSizes: true; spacing: 4
        Rectangle { objectName: "rl5a"; implicitWidth: 10; implicitHeight: 10 }
        Rectangle { objectName: "rl5b"; implicitWidth: 30; implicitHeight: 10; Layout.fillWidth: true }
        Rectangle { objectName: "rl5c"; implicitWidth: 20; implicitHeight: 10 }
    }
    RowLayout { objectName: "rl6"; x: 0; y: 200; width: 200; layoutDirection: Qt.RightToLeft
        Rectangle { objectName: "rl6a"; implicitWidth: 10; implicitHeight: 10 }
        Rectangle { objectName: "rl6b"; implicitWidth: 30; implicitHeight: 10 }
        ColumnLayout { objectName: "rl6col"; spacing: 1
            Rectangle { objectName: "rl6c1"; implicitWidth: 12; implicitHeight: 4 }
            Rectangle { objectName: "rl6c2"; implicitWidth: 8; implicitHeight: 4; Layout.fillWidth: true }
        }
    }
    RowLayout { objectName: "rl7"; x: 0; y: 240; width: 101; height: 11; spacing: 0
        Rectangle { objectName: "rl7a"; Layout.fillWidth: true; Layout.fillHeight: true }
        Rectangle { objectName: "rl7b"; Layout.fillWidth: true; Layout.fillHeight: true }
        Rectangle { objectName: "rl7c"; Layout.fillWidth: true; Layout.fillHeight: true }
    }
    RowLayout { objectName: "rl8"; anchors.right: parent.right; y: 260
        Rectangle { objectName: "rl8a"; implicitWidth: 15; implicitHeight: 15; Layout.fillWidth: false }
        Loader { objectName: "rl8ld"; active: true; sourceComponent: Rectangle { implicitWidth: 22; implicitHeight: 9 } }
        Loader { objectName: "rl8ldOff"; active: false; sourceComponent: Rectangle { implicitWidth: 22; implicitHeight: 9 } }
        Loader { objectName: "rl8ldFill"; Layout.fillHeight: true; Layout.preferredWidth: 6; sourceComponent: Rectangle { implicitWidth: 22; implicitHeight: 9 } }
    }
}
