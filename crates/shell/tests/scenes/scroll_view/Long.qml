import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The input field of AssistantChat.qml: a TextArea that wraps and fills its parent in a
// ScrollView a RowLayout makes as wide as it has room; its text is set whole.
Item {
    id: root
    width: 400; height: 300

    Rectangle {
        id: wrapper
        width: 360; height: 120
        RowLayout {
            id: row
            anchors { bottom: parent.bottom; left: parent.left; right: parent.right }
            spacing: 0
            ScrollView {
                id: view
                Layout.fillWidth: true
                Layout.preferredHeight: Math.min(root.height * 3 / 5, Math.max(field.height, 0))
                clip: true
                TextArea {
                    id: field
                    font.family: "DejaVu Sans"; font.pixelSize: 14
                    anchors.fill: parent
                    wrapMode: TextArea.Wrap
                    padding: 10
                    background: null
                    Text {
                        id: hint
                        visible: field.text.length === 0
                        x: field.leftPadding; y: field.topPadding
                        width: view.availableWidth - field.leftPadding - field.rightPadding
                        wrapMode: Text.Wrap
                        font: field.font
                        text: "Message the assistant..."
                    }
                }
            }
            Rectangle { id: send; implicitWidth: 40; implicitHeight: 40 }
        }
    }
    // the same without the anchors
    ScrollView {
        id: view2
        y: 150; width: 320
        TextArea { id: field2; font.family: "DejaVu Sans"; font.pixelSize: 14; wrapMode: TextArea.Wrap; padding: 10; background: null }
    }

    function type() { field.text = "more 1"; field2.text = "more 1" }
    function more() { field.text = "more 1 and then some more words that do not fit in one line of the field at all"; field2.text = field.text }

    readonly property var names: ["implicitWidth", "implicitHeight", "width", "height", "contentWidth",
        "contentHeight", "lineCount", "availableWidth"]
    function rowOf(n, o) {
        var s = n + ":"
        for (var i = 0; i < names.length; i++) {
            var v = o[names[i]]
            s += " " + (v === undefined ? "-" : Math.round(v * 100) / 100)
        }
        return s
    }
    function snap() {
        var all = { view: view, field: field, view2: view2, field2: field2 }
        var out = []
        for (var k in all)
            out.push(rowOf(k, all[k]))
        return out.join("\n")
    }
}
