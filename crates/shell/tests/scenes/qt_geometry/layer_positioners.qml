import QtQuick
import QtQuick.Layouts
Item {
    id: root; width: 400; height: 400
    // The items made from `layer.effect` of row1b and rl1b.
    property Item rowFx: null
    property Item layoutFx: null
    Row { objectName: "row1"; spacing: 1
        Rectangle { width: 10; height: 20; layer.enabled: true; objectName: "row1a"; layer.effect: ShaderEffect {} }
        Rectangle { width: 10; height: 20; layer.enabled: true; objectName: "row1b"; layer.effect: ShaderEffect { Component.onCompleted: root.rowFx = this } }
        Rectangle { objectName: "row1c"; width: 5; height: 5 }
    }
    Column { objectName: "col1"; y: 40; spacing: 2
        Rectangle { width: 10; height: 20; layer.enabled: true; objectName: "col1a"; layer.effect: ShaderEffect {} }
        Rectangle { width: 10; height: 20; layer.enabled: true; objectName: "col1b"; layer.effect: null }
        Rectangle { objectName: "col1c"; width: 5; height: 5 }
    }
    Grid { objectName: "grid1"; y: 120; columns: 2; spacing: 1
        Rectangle { width: 10; height: 20; layer.enabled: true; objectName: "grid1a"; layer.effect: ShaderEffect {} }
        Rectangle { width: 10; height: 20; layer.enabled: true; objectName: "grid1b"; layer.effect: ShaderEffect {} }
        Rectangle { width: 10; height: 20; layer.enabled: true; objectName: "grid1c"; layer.effect: ShaderEffect {} }
    }
    Flow { objectName: "flow1"; y: 180; width: 25; spacing: 1
        Rectangle { width: 10; height: 20; layer.enabled: true; objectName: "flow1a"; layer.effect: ShaderEffect {} }
        Rectangle { width: 10; height: 20; layer.enabled: true; objectName: "flow1b"; layer.effect: ShaderEffect {} }
        Rectangle { width: 10; height: 20; layer.enabled: true; objectName: "flow1c"; layer.effect: ShaderEffect {} }
    }
    RowLayout { objectName: "rl1"; y: 250; spacing: 3
        Rectangle { width: 10; height: 20; layer.enabled: true; objectName: "rl1a"; implicitWidth: 10; implicitHeight: 20; layer.effect: ShaderEffect {} }
        Rectangle { width: 10; height: 20; layer.enabled: true; objectName: "rl1b"; implicitWidth: 12; implicitHeight: 20; layer.effect: ShaderEffect { Component.onCompleted: root.layoutFx = this } }
        Rectangle { objectName: "rl1c"; implicitWidth: 5; implicitHeight: 5 }
    }
    ColumnLayout { objectName: "cl1"; y: 300; spacing: 3
        Rectangle { width: 10; height: 20; layer.enabled: true; objectName: "cl1a"; implicitWidth: 10; implicitHeight: 20; layer.effect: ShaderEffect {} }
        Rectangle { width: 10; height: 20; layer.enabled: true; objectName: "cl1b"; implicitWidth: 10; implicitHeight: 20; layer.effect: ShaderEffect {} }
        Rectangle { objectName: "cl1c"; implicitWidth: 5; implicitHeight: 5 }
    }
    Item { objectName: "plain"; x: 200; width: childrenRect.width; height: childrenRect.height
        Rectangle { width: 10; height: 20; layer.enabled: true; objectName: "plain_a"; x: 4; y: 5; layer.effect: ShaderEffect {} }
        Rectangle { width: 10; height: 20; layer.enabled: true; objectName: "plain_b"; x: 30; y: 40; layer.effect: ShaderEffect {} }
    }
    // The effect items are where their items are.
    Item {
        objectName: "row_fx_at"
        x: root.rowFx ? root.rowFx.x : -1; y: root.rowFx ? root.rowFx.y : -1
        width: root.rowFx ? root.rowFx.width : -1; height: root.rowFx ? root.rowFx.height : -1
    }
    Item {
        objectName: "layout_fx_at"
        x: root.layoutFx ? root.layoutFx.x : -1; y: root.layoutFx ? root.layoutFx.y : -1
        width: root.layoutFx ? root.layoutFx.width : -1; height: root.layoutFx ? root.layoutFx.height : -1
    }
}
