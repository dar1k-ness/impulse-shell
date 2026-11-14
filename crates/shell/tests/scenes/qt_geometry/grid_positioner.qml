import QtQuick
Item {
    id: root; width: 400; height: 400
    Grid { objectName: "grid1"; columns: 3; spacing: 2; horizontalItemAlignment: Grid.AlignHCenter; verticalItemAlignment: Grid.AlignBottom
        Repeater { model: 7; Rectangle { objectName: "grid1_" + index; width: 10 + index * 3; height: 5 + (index % 3) * 4 } }
    }
    Grid { objectName: "grid2"; x: 200; rows: 2; rowSpacing: 1; columnSpacing: 6; flow: Grid.TopToBottom
        Repeater { model: 5; Rectangle { objectName: "grid2_" + index; width: 10; height: 10 } }
    }
    Grid { objectName: "grid3"; y: 80
        Repeater { model: 6; Rectangle { objectName: "grid3_" + index; width: 8 + index; height: 8 } }
    }
    Grid { objectName: "grid4"; y: 120; rows: 2; spacing: 1; horizontalItemAlignment: Grid.AlignRight; verticalItemAlignment: Grid.AlignVCenter
        Repeater { model: 5; Rectangle { objectName: "grid4_" + index; width: 10 + index * 2; height: 6 + index * 2 } }
    }
    Grid { objectName: "grid5"; y: 170; columns: 2; spacing: 3; padding: 4; layoutDirection: Qt.RightToLeft
        Repeater { model: 3; Rectangle { objectName: "grid5_" + index; width: 10 + index * 5; height: 10 } }
    }
}
