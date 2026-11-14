// The field of qml/modules/rusty/sidebarRight/todo/TodoWidget.qml:169-197 with active focus, its
// colours written out. `todo.png` is the picture Qt 6.12 draws of it (`dump.py`).
import QtQuick
import QtQuick.Controls

Item {
    width: 260; height: 70

    Rectangle { anchors.fill: parent; color: "#808080" }

    TextField {
        id: todoInput
        objectName: "field"
        x: 10; y: 10; width: 240
        padding: 10
        color: activeFocus ? "white" : "#d0d0d0"
        renderType: Text.NativeRendering
        selectedTextColor: "black"
        selectionColor: "#c0e0ff"
        placeholderText: "Task description"
        placeholderTextColor: "#b0b0b0"
        focus: true
        text: "Buy milk"

        background: Rectangle {
            anchors.fill: parent
            radius: 4
            border.width: 2
            border.color: todoInput.activeFocus ? "#ffd040" : "#b0b0b0"
            color: "transparent"
        }

        cursorDelegate: Rectangle {
            width: 1
            color: todoInput.activeFocus ? "#ffd040" : "transparent"
            radius: 1
        }
    }
}
