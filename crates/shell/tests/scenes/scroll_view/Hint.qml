import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The input of AssistantChat.qml with its hint, a Text in the field. The field of a ScrollView
// is as wide as its own text, so a hint as wide as the field (`a`) has no width and stays on one
// line; one as wide as the view (`b`) wraps, and the view grows by its lines.
Item {
    id: root
    width: 336; height: 400

    property string agentName: "Agent"
    function longName() { root.agentName = "old-agent-with-a-rather-long-program-name-acp"; }
    function type() { at.text = "x"; bt.text = "x"; }

    function snap() {
        const r = v => Math.round(v * 100) / 100;
        const one = (n, v, t, h) => n + ": view " + r(v.width) + "x" + r(v.height)
            + " content " + r(v.contentWidth) + "x" + r(v.contentHeight)
            + " flickable " + r(v.contentItem.width) + "x" + r(v.contentItem.height)
            + " field " + r(t.x) + "," + r(t.y) + " " + r(t.width) + "x" + r(t.height)
            + " implicit " + r(t.implicitWidth) + "x" + r(t.implicitHeight)
            + " hint " + r(h.width) + "x" + r(h.height) + " lines " + h.lineCount + " visible " + h.visible;
        return one("a", a, at, ah) + "\n" + one("b", b, bt, bh);
    }

    RowLayout {
        y: 0; width: parent.width
        spacing: 0
        ScrollView {
            id: a
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(root.height * 3 / 5, Math.max(at.height,
                ah.visible && ah.lineCount > 1 ? ah.height + at.topPadding + at.bottomPadding : 0))
            clip: true
            ScrollBar.vertical.policy: ScrollBar.AsNeeded
            TextArea {
                id: at
                anchors.fill: parent
                wrapMode: TextEdit.Wrap
                padding: 10
                font.family: "DejaVu Sans"; font.pixelSize: 14
                background: null
                Text {
                    id: ah
                    visible: at.text.length === 0 && !at.preeditText
                    x: at.leftPadding
                    y: at.topPadding
                    width: at.width - at.leftPadding - at.rightPadding
                    wrapMode: Text.Wrap
                    font: at.font
                    text: "Message %1...".arg(root.agentName)
                }
            }
        }
        Item { implicitWidth: 40; implicitHeight: 40; Layout.rightMargin: 5 }
    }

    RowLayout {
        y: 200; width: parent.width
        spacing: 0
        ScrollView {
            id: b
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(root.height * 3 / 5, Math.max(bt.height,
                bh.visible && bh.lineCount > 1 ? bh.height + bt.topPadding + bt.bottomPadding : 0))
            clip: true
            ScrollBar.vertical.policy: ScrollBar.AsNeeded
            TextArea {
                id: bt
                anchors.fill: parent
                wrapMode: TextEdit.Wrap
                padding: 10
                font.family: "DejaVu Sans"; font.pixelSize: 14
                background: null
                Text {
                    id: bh
                    visible: bt.text.length === 0 && !bt.preeditText
                    x: bt.leftPadding
                    y: bt.topPadding
                    width: b.availableWidth - bt.leftPadding - bt.rightPadding
                    wrapMode: Text.Wrap
                    font: bt.font
                    text: "Message %1...".arg(root.agentName)
                }
            }
        }
        Item { implicitWidth: 40; implicitHeight: 40; Layout.rightMargin: 5 }
    }
}
