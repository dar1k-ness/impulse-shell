// cell 260x90 cols 3: ProgressIndeterminate Progress40 TextField TextFieldText TextArea WProgress
import QtQuick
import QtQuick.Controls.Material
import QtQuick.Controls

Grid {
    columns: 3
    component Cell: Item { width: 260; height: 90 }
    // qml/modules/common/widgets/StyledIndeterminateProgressBar.qml:6
    Cell { ProgressBar { x: 10; y: 40; width: 240; indeterminate: true; Material.accent: "#80a0ff" } }
    Cell { ProgressBar { x: 10; y: 40; width: 240; value: 0.4; Material.accent: "#80a0ff" } }
    // qml/modules/common/widgets/MaterialTextField.qml:11
    Cell {
        TextField {
            x: 10; y: 10; width: 240
            Material.theme: Material.System; Material.accent: "#80a0ff"; Material.primary: "#80a0ff"
            Material.background: "#202020"; Material.foreground: "#e0e0e0"
            Material.containerStyle: Material.Outlined
            renderType: Text.QtRendering; font.pixelSize: 15
            selectedTextColor: "black"; selectionColor: "#c0e0ff"; placeholderTextColor: "#d0d0d0"
            placeholderText: "Password"
        }
    }
    Cell {
        TextField {
            x: 10; y: 10; width: 240
            Material.theme: Material.System; Material.accent: "#80a0ff"; Material.primary: "#80a0ff"
            Material.background: "#202020"; Material.foreground: "#e0e0e0"
            Material.containerStyle: Material.Outlined
            renderType: Text.QtRendering; font.pixelSize: 15
            selectedTextColor: "black"; selectionColor: "#c0e0ff"; placeholderTextColor: "#d0d0d0"
            placeholderText: "Password"; text: "secret"
        }
    }
    // qml/modules/common/widgets/MaterialTextArea.qml:11
    Cell {
        TextArea {
            x: 10; y: 10; width: 240
            Material.theme: Material.System; Material.accent: "#80a0ff"; Material.primary: "#80a0ff"
            Material.background: "#202020"; Material.foreground: "#e0e0e0"
            Material.containerStyle: Material.Filled
            renderType: Text.QtRendering; font.pixelSize: 15
            selectedTextColor: "black"; selectionColor: "#c0e0ff"; placeholderTextColor: "#d0d0d0"
            placeholderText: "Name"
            background: Rectangle { implicitHeight: 56; color: "#404040"; radius: 4 }
        }
    }
    // qml/modules/waffle/looks/WIndeterminateProgressBar.qml:9 (background null, contentItem of Material)
    Cell { ProgressBar { x: 10; y: 40; width: 240; indeterminate: true; implicitHeight: 3; background: null; Material.accent: "#80a0ff" } }
}
