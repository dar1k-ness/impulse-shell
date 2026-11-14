import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The input of AssistantChat.qml typed into, key by key: the field of a ScrollView is as wide
// as its text on one line, so two words typed stay on one line.
Item {
    id: root
    width: 336; height: 200

    function start() { t.forceActiveFocus(); }
    function snap() {
        const r = v => Math.round(v * 100) / 100;
        return "view " + r(v.width) + "x" + r(v.height)
            + " content " + r(v.contentWidth) + "x" + r(v.contentHeight)
            + " field " + r(t.width) + "x" + r(t.height)
            + " implicit " + r(t.implicitWidth) + "x" + r(t.implicitHeight)
            + " text " + r(t.contentWidth) + "x" + r(t.contentHeight) + " lines " + t.lineCount
            + " hint " + h.visible;
    }

    RowLayout {
        width: parent.width
        spacing: 0
        ScrollView {
            id: v
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(root.height * 3 / 5, Math.max(t.height,
                h.visible && h.lineCount > 1 ? h.height + t.topPadding + t.bottomPadding : 0))
            clip: true
            ScrollBar.vertical.policy: ScrollBar.AsNeeded
            TextArea {
                id: t
                anchors.fill: parent
                wrapMode: TextEdit.Wrap
                padding: 10
                font.family: "DejaVu Sans"; font.pixelSize: 14
                background: null
                Text {
                    id: h
                    visible: t.text.length === 0 && !t.preeditText
                    x: t.leftPadding
                    y: t.topPadding
                    width: v.availableWidth - t.leftPadding - t.rightPadding
                    wrapMode: Text.Wrap
                    font: t.font
                    text: "Message Agent..."
                }
            }
        }
        Item { implicitWidth: 40; implicitHeight: 40; Layout.rightMargin: 5 }
    }
}
