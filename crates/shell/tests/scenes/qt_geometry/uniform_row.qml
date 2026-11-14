import QtQuick
import QtQuick.Layouts
Item {
    id: root; width: 400; height: 400
    RowLayout { objectName: "u1"; uniformCellSizes: true; spacing: 4
        Rectangle { objectName: "u1a"; implicitWidth: 10; implicitHeight: 10 }
        Rectangle { objectName: "u1b"; implicitWidth: 30; implicitHeight: 10; Layout.fillWidth: true }
        Rectangle { objectName: "u1c"; implicitWidth: 20; implicitHeight: 10 }
    }
    RowLayout { objectName: "u2"; y: 20; uniformCellSizes: true; spacing: 4
        Rectangle { objectName: "u2a"; implicitWidth: 10; implicitHeight: 10; Layout.fillWidth: true }
        Rectangle { objectName: "u2b"; implicitWidth: 30; implicitHeight: 10; Layout.fillWidth: true }
        Rectangle { objectName: "u2c"; implicitWidth: 20; implicitHeight: 10; Layout.fillWidth: true }
    }
    RowLayout { objectName: "u3"; y: 40; uniformCellSizes: true; spacing: 4; width: 300
        Rectangle { objectName: "u3a"; implicitWidth: 10; implicitHeight: 10 }
        Rectangle { objectName: "u3b"; implicitWidth: 30; implicitHeight: 10 }
        Rectangle { objectName: "u3c"; implicitWidth: 20; implicitHeight: 10; Layout.fillWidth: true }
    }
    ColumnLayout { objectName: "u4"; y: 70; uniformCellSizes: true; spacing: 2
        Rectangle { objectName: "u4a"; implicitWidth: 10; implicitHeight: 10 }
        Rectangle { objectName: "u4b"; implicitWidth: 10; implicitHeight: 40; Layout.fillHeight: true }
        Rectangle { objectName: "u4c"; implicitWidth: 10; implicitHeight: 16 }
    }
}
