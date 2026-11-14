import QtQuick
Item {
    id: root; width: 400; height: 400
    Row { objectName: "row1"; spacing: 5; padding: 3; leftPadding: 7
        Rectangle { objectName: "row1a"; width: 10; height: 10 }
        Rectangle { objectName: "row1b"; width: 20; height: 30; visible: false }
        Rectangle { objectName: "row1c"; width: 0; height: 12 }
        Rectangle { objectName: "row1d"; width: 15; height: 0 }
        Item { objectName: "row1e"; implicitWidth: 9; implicitHeight: 9 }
        Rectangle { objectName: "row1f"; width: 11; height: 11; opacity: 0 }
    }
    Column { objectName: "col1"; y: 50; spacing: 2; bottomPadding: 4
        Rectangle { objectName: "col1a"; width: 10; height: 10 }
        Repeater { model: 3; Rectangle { objectName: "col1r" + index; width: 5 * index; height: 5 * index } }
        Rectangle { objectName: "col1z"; width: parent.width; height: 4 }
        Row { objectName: "col1row"; Rectangle { objectName: "col1rowa"; width: 33; height: 6 } }
    }
    Column { objectName: "col3"; x: 100; y: 50; spacing: 2; padding: 5; rightPadding: 9; topPadding: 1
        Rectangle { objectName: "col3a"; width: 10; height: 10 }
        Rectangle { objectName: "col3b"; width: 24; height: 6 }
    }
    Flow { objectName: "flow1"; y: 220; width: 100; spacing: 4; padding: 2
        Repeater { model: 8; Rectangle { objectName: "flow1_" + index; width: 20 + index * 4; height: 10 + index } }
    }
    Row { objectName: "row2"; y: 330; layoutDirection: Qt.RightToLeft; spacing: 1; width: 100
        Rectangle { objectName: "row2a"; width: 10; height: 10 }
        Rectangle { objectName: "row2b"; width: 20; height: 10 }
    }
    Row { objectName: "row3"; x: 200; y: 330; layoutDirection: Qt.RightToLeft; spacing: 1; leftPadding: 6; rightPadding: 2
        Rectangle { objectName: "row3a"; width: 10; height: 10 }
        Rectangle { objectName: "row3b"; width: 20; height: 10 }
    }
    Flow { objectName: "flow3"; y: 350; width: 60; layoutDirection: Qt.RightToLeft; spacing: 2
        Repeater { model: 4; Rectangle { objectName: "flow3_" + index; width: 25; height: 6 } }
    }
    Flow { objectName: "flow4"; x: 200; y: 350; width: 70; layoutDirection: Qt.RightToLeft; spacing: 2; leftPadding: 8; rightPadding: 3; topPadding: 1
        Repeater { model: 4; Rectangle { objectName: "flow4_" + index; width: 25; height: 6 } }
    }
}
