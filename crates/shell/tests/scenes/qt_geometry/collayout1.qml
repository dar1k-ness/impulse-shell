import QtQuick
import QtQuick.Layouts
Item {
    id: root; width: 400; height: 400
    ColumnLayout { objectName: "cl1"; anchors.fill: parent; anchors.margins: 10; spacing: 3
        Rectangle { objectName: "cl1a"; implicitWidth: 30; implicitHeight: 10 }
        Rectangle { objectName: "cl1b"; Layout.fillWidth: true; implicitHeight: 20 }
        Rectangle { objectName: "cl1c"; Layout.fillHeight: true; Layout.alignment: Qt.AlignHCenter; implicitWidth: 44 }
        RowLayout { objectName: "cl1row"; Layout.alignment: Qt.AlignRight
            Rectangle { objectName: "cl1r1"; implicitWidth: 10; implicitHeight: 10 }
            Rectangle { objectName: "cl1r2"; implicitWidth: 10; implicitHeight: 14 }
        }
        RowLayout { objectName: "cl1row2"; Layout.fillWidth: false
            Rectangle { objectName: "cl1r3"; implicitWidth: 10; implicitHeight: 10; Layout.fillWidth: true }
        }
        Item { objectName: "cl1item"; implicitWidth: 50; implicitHeight: 5; Layout.fillWidth: true; Layout.maximumWidth: 120; Layout.alignment: Qt.AlignRight }
        Rectangle { objectName: "cl1z"; Layout.fillHeight: true; Layout.preferredHeight: 50; Layout.minimumHeight: 20; Layout.fillWidth: true; Layout.leftMargin: 20; Layout.rightMargin: 5 }
        Repeater { model: 2; Rectangle { objectName: "cl1rep" + index; implicitWidth: 7 + index; implicitHeight: 7 } }
    }
}
