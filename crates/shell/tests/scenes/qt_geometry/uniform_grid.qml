import QtQuick
import QtQuick.Layouts
Item {
    id: root; width: 400; height: 400
    GridLayout { objectName: "w1"; columns: 2; rowSpacing: 5; columnSpacing: 5; uniformCellWidths: true
        Rectangle { objectName: "w1a"; implicitWidth: 40; implicitHeight: 10 }
        Rectangle { objectName: "w1b"; implicitWidth: 80; implicitHeight: 10 }
        Rectangle { objectName: "w1c"; implicitWidth: 60; implicitHeight: 10; Layout.fillWidth: true }
        Rectangle { objectName: "w1d"; implicitWidth: 20; implicitHeight: 10; Layout.fillWidth: true }
    }
    GridLayout { objectName: "w2"; y: 50; columns: 2; rows: 2; columnSpacing: 0; rowSpacing: 0; uniformCellWidths: true; uniformCellHeights: true; width: 200; height: 100
        Repeater { model: 3; Rectangle { objectName: "w2_" + index; implicitWidth: 10 + index * 10; implicitHeight: 10 + index * 5 } }
    }
    GridLayout { objectName: "g3"; y: 200; width: 210; columns: 3; uniformCellWidths: true; columnSpacing: 0
        Rectangle { objectName: "g3a"; implicitWidth: 10; implicitHeight: 10 }
        Rectangle { objectName: "g3b"; implicitWidth: 50; implicitHeight: 10; Layout.fillWidth: true }
        Rectangle { objectName: "g3c"; implicitWidth: 20; implicitHeight: 10 }
        Rectangle { objectName: "g3d"; implicitWidth: 20; implicitHeight: 10; visible: false }
        Rectangle { objectName: "g3e"; implicitWidth: 20; implicitHeight: 10 }
    }
    GridLayout { objectName: "h1"; y: 260; columns: 2; rowSpacing: 3; uniformCellHeights: true
        Rectangle { objectName: "h1a"; implicitWidth: 10; implicitHeight: 10 }
        Rectangle { objectName: "h1b"; implicitWidth: 10; implicitHeight: 30 }
        Rectangle { objectName: "h1c"; implicitWidth: 10; implicitHeight: 14; Layout.fillHeight: true }
        Rectangle { objectName: "h1d"; implicitWidth: 10; implicitHeight: 6 }
    }
    GridLayout { objectName: "o1"; y: 340; columns: 2; rowSpacing: 5; columnSpacing: 5; uniformCellWidths: true
        Rectangle { objectName: "o1a"; implicitWidth: 107; implicitHeight: 10; Layout.fillWidth: true }
        Rectangle { objectName: "o1b"; implicitWidth: 116; implicitHeight: 10; Layout.fillWidth: true }
        Rectangle { objectName: "o1c"; implicitWidth: 50; implicitHeight: 10; Layout.fillWidth: true }
        Rectangle { objectName: "o1d"; implicitWidth: 60; implicitHeight: 10; Layout.fillWidth: true }
    }
}
