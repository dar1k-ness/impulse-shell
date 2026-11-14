import QtQuick
import QtQuick.Layouts
Item {
    id: root; width: 400; height: 400
    GridLayout { objectName: "s1"; columns: 3; width: 300
        Rectangle { objectName: "s1a"; implicitWidth: 10; implicitHeight: 10; Layout.columnSpan: 2; Layout.fillWidth: true }
        Rectangle { objectName: "s1b"; implicitWidth: 30; implicitHeight: 10 }
        Rectangle { objectName: "s1c"; implicitWidth: 20; implicitHeight: 10 }
        Rectangle { objectName: "s1d"; implicitWidth: 20; implicitHeight: 10 }
    }
    GridLayout { objectName: "g2"; y: 40; width: 200; height: 80; columns: 2
        Rectangle { objectName: "g2a"; Layout.fillWidth: true; implicitHeight: 10 }
        Rectangle { objectName: "g2b"; implicitWidth: 30; implicitHeight: 10 }
        Rectangle { objectName: "g2c"; implicitWidth: 30; implicitHeight: 10; Layout.alignment: Qt.AlignRight | Qt.AlignBottom }
        Rectangle { objectName: "g2d"; Layout.fillWidth: true; Layout.fillHeight: true }
        Rectangle { objectName: "g2e"; Layout.columnSpan: 2; Layout.fillWidth: true; implicitHeight: 6 }
    }
    GridLayout { objectName: "g4"; y: 130; rows: 2; flow: GridLayout.TopToBottom
        Repeater { model: 5; Rectangle { objectName: "g4_" + index; implicitWidth: 10 + index; implicitHeight: 10 } }
    }
    GridLayout { objectName: "rs"; y: 170; columns: 3; rowSpacing: 2; columnSpacing: 2
        Rectangle { objectName: "rsa"; implicitWidth: 10; implicitHeight: 10; Layout.rowSpan: 2; Layout.fillHeight: true }
        Rectangle { objectName: "rsb"; implicitWidth: 20; implicitHeight: 10 }
        Rectangle { objectName: "rsc"; implicitWidth: 30; implicitHeight: 10 }
        Rectangle { objectName: "rsd"; implicitWidth: 40; implicitHeight: 14 }
        Rectangle { objectName: "rse"; implicitWidth: 15; implicitHeight: 10 }
        Rectangle { objectName: "rsf"; implicitWidth: 12; implicitHeight: 10 }
    }
    GridLayout { objectName: "rc"; y: 220; columns: 3; rowSpacing: 1; columnSpacing: 1
        Rectangle { objectName: "rca"; implicitWidth: 10; implicitHeight: 10; Layout.row: 1; Layout.column: 2 }
        Rectangle { objectName: "rcb"; implicitWidth: 20; implicitHeight: 12 }
        Rectangle { objectName: "rcc"; implicitWidth: 30; implicitHeight: 14; Layout.column: 1; Layout.row: 0 }
        Rectangle { objectName: "rcd"; implicitWidth: 16; implicitHeight: 8 }
    }
    GridLayout { objectName: "tb"; y: 270; rows: 3; flow: GridLayout.TopToBottom; rowSpacing: 1; columnSpacing: 1
        Rectangle { objectName: "tba"; implicitWidth: 10; implicitHeight: 10; Layout.rowSpan: 2; Layout.fillHeight: true }
        Rectangle { objectName: "tbb"; implicitWidth: 20; implicitHeight: 10 }
        Rectangle { objectName: "tbc"; implicitWidth: 30; implicitHeight: 12; Layout.columnSpan: 2; Layout.fillWidth: true }
        Rectangle { objectName: "tbd"; implicitWidth: 14; implicitHeight: 9 }
        Rectangle { objectName: "tbe"; implicitWidth: 18; implicitHeight: 7 }
    }
}
