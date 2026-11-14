// cell 260x130 cols 3: AssistantChat433 AssistantChat433active NotesContent147active MessageCodeBlock186 ToolbarTextField TodoWidget169
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Grid {
    columns: 3
    component Cell: Item { width: 260; height: 130 }
    readonly property string longText: "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\nline 11\nline 12"
    // qml/modules/rusty/sidebarLeft/AssistantChat.qml:433
    Cell {
        ColumnLayout {
            x: 10; y: 10; width: 240
            ScrollView {
                Layout.fillWidth: true
                Layout.preferredHeight: 100
                clip: true
                ScrollBar.vertical.policy: ScrollBar.AsNeeded
                TextArea { wrapMode: TextArea.Wrap; padding: 10; color: "white"; background: null; text: longText }
            }
        }
    }
    // The same while the pointer is over it or it scrolls (ScrollBar.active).
    Cell {
        ColumnLayout {
            x: 10; y: 10; width: 240
            ScrollView {
                id: sv2
                Layout.fillWidth: true
                Layout.preferredHeight: 100
                clip: true
                ScrollBar.vertical.policy: ScrollBar.AsNeeded
                TextArea { wrapMode: TextArea.Wrap; padding: 10; color: "white"; background: null; text: longText }
                Component.onCompleted: sv2.ScrollBar.vertical.active = true
            }
        }
    }
    // qml/modules/rusty/overlay/notes/NotesContent.qml:147, active
    Cell {
        ColumnLayout {
            x: 10; y: 10; width: 240; height: 110
            ScrollView {
                id: sv3
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                ScrollBar.vertical.policy: ScrollBar.AsNeeded
                TextArea {
                    anchors { left: parent.left; right: parent.right }
                    wrapMode: TextEdit.Wrap; color: "white"; background: null; padding: 16; text: longText
                }
                Component.onCompleted: sv3.ScrollBar.vertical.active = true
            }
        }
    }
    // qml/modules/rusty/sidebarLeft/aiChat/MessageCodeBlock.qml:186
    Cell {
        ColumnLayout {
            x: 10; y: 10; width: 240
            ScrollView {
                Layout.fillWidth: true
                implicitWidth: parent.width
                implicitHeight: code.implicitHeight + 1
                contentWidth: code.width - 1
                clip: true
                ScrollBar.vertical.policy: ScrollBar.AlwaysOff
                ScrollBar.horizontal: ScrollBar {
                    anchors.bottom: parent.bottom
                    anchors.left: parent.left
                    anchors.right: parent.right
                    padding: 5
                    policy: ScrollBar.AsNeeded
                    opacity: visualSize == 1 ? 0 : 1
                    visible: opacity > 0
                    contentItem: Rectangle { implicitHeight: 6; radius: 3; color: "#ffcc00" }
                }
                TextArea {
                    id: code
                    Layout.fillWidth: true
                    readOnly: true
                    color: "white"
                    selectedTextColor: "black"; selectionColor: "#c0e0ff"
                    text: "a line of code that is a good deal wider than the block that shows it\nsecond line"
                }
            }
        }
    }
    // qml/modules/common/widgets/ToolbarTextField.qml:7 (caret, selection, placeholder)
    Cell {
        TextField {
            x: 10; y: 10; implicitWidth: 200; padding: 10
            placeholderTextColor: "#c0c0c0"; color: "white"
            selectedTextColor: "black"; selectionColor: "#c0e0ff"
            placeholderText: "Search"
            background: Rectangle { color: "#404040"; radius: 8 }
        }
        TextField {
            x: 10; y: 70; implicitWidth: 200; padding: 10
            color: "white"; selectedTextColor: "black"; selectionColor: "#c0e0ff"
            text: "typed text"; cursorVisible: true
            background: Rectangle { color: "#404040"; radius: 8 }
            Component.onCompleted: select(0, 5)
        }
    }
    // qml/modules/rusty/sidebarRight/todo/TodoWidget.qml:169 (its own cursorDelegate)
    Cell {
        TextField {
            x: 10; y: 10; width: 200; padding: 10
            color: "white"; placeholderText: "Task description"; placeholderTextColor: "#c0c0c0"
            selectedTextColor: "black"; selectionColor: "#c0e0ff"
            cursorVisible: true
            background: Rectangle { color: "#404040"; radius: 4; border.width: 2; border.color: "#80a0ff" }
            cursorDelegate: Rectangle { width: 1; color: "#80a0ff"; radius: 1 }
        }
    }
}
