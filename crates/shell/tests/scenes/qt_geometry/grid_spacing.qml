import QtQuick
Item {
    id: root; width: 400; height: 400
    Grid { objectName: "none"; columns: 2; Repeater { model: 4; Rectangle { objectName: parent.objectName + "_" + index; width: 10; height: 10 } } }
    Grid { objectName: "spacing"; x: 50; columns: 2; spacing: 4; Repeater { model: 4; Rectangle { objectName: parent.objectName + "_" + index; width: 10; height: 10 } } }
    Grid { objectName: "row"; x: 100; columns: 2; rowSpacing: 3; Repeater { model: 4; Rectangle { objectName: parent.objectName + "_" + index; width: 10; height: 10 } } }
    Grid { objectName: "column"; x: 150; columns: 2; columnSpacing: 5; Repeater { model: 4; Rectangle { objectName: parent.objectName + "_" + index; width: 10; height: 10 } } }
    Grid { objectName: "spacing_row"; x: 200; columns: 2; spacing: 4; rowSpacing: 3; Repeater { model: 4; Rectangle { objectName: parent.objectName + "_" + index; width: 10; height: 10 } } }
    Grid { objectName: "spacing_column0"; x: 250; columns: 2; spacing: 4; columnSpacing: 0; Repeater { model: 4; Rectangle { objectName: parent.objectName + "_" + index; width: 10; height: 10 } } }
    // -1 is the stored value: writing it is no write, the row gap stays `spacing`.
    Grid { objectName: "row_minus1"; x: 300; columns: 2; spacing: 4; rowSpacing: -1; Repeater { model: 4; Rectangle { objectName: parent.objectName + "_" + index; width: 10; height: 10 } } }
    // Written after another value, -1 is the gap.
    Grid { objectName: "row_back_to_minus1"; y: 50; columns: 2; spacing: 4; Repeater { model: 4; Rectangle { objectName: parent.objectName + "_" + index; width: 10; height: 10 } }
        Component.onCompleted: { rowSpacing = 0; rowSpacing = -1 }
    }
    // Reset: the row gap is `spacing` again.
    Grid { objectName: "row_reset"; x: 50; y: 50; columns: 2; spacing: 4; Repeater { model: 4; Rectangle { objectName: parent.objectName + "_" + index; width: 10; height: 10 } }
        Component.onCompleted: { rowSpacing = 2; rowSpacing = undefined; spacing = 6 }
    }
    // The reset keeps the stored value, so writing it again is no write.
    Grid { objectName: "column_reset_same"; x: 100; y: 50; columns: 2; spacing: 4; Repeater { model: 4; Rectangle { objectName: parent.objectName + "_" + index; width: 10; height: 10 } }
        Component.onCompleted: { columnSpacing = 3; columnSpacing = undefined; columnSpacing = 3; spacing = 5 }
    }
    Grid { objectName: "column_reset_other"; x: 150; y: 50; columns: 2; spacing: 4; Repeater { model: 4; Rectangle { objectName: parent.objectName + "_" + index; width: 10; height: 10 } }
        Component.onCompleted: { columnSpacing = 3; columnSpacing = undefined; columnSpacing = 1; spacing = 5 }
    }
    Grid { objectName: "negative"; x: 200; y: 50; columns: 2; rowSpacing: -5; columnSpacing: -2; Repeater { model: 4; Rectangle { objectName: parent.objectName + "_" + index; width: 10; height: 10 } } }
}
