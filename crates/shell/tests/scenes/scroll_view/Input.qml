import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The two editors of qml/ under the pointer and the keys: the input of AssistantChat.qml (`a`)
// and the editor of NotesContent.qml (`b`). `Input.steps` is what is done to them, `snap()`
// where each view is and how its vertical bar shows after each step.
Item {
    id: root
    width: 260; height: 280

    readonly property string lines: "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\neleven\ntwelve\nthirteen\nfourteen\nfifteen\nsixteen"

    Item {
        width: 260; height: 130
        ColumnLayout {
            anchors.fill: parent
            spacing: 0
            ScrollView {
                id: a
                Layout.fillWidth: true
                Layout.preferredHeight: Math.min(parent.height * 3 / 5, at.height)
                clip: true
                ScrollBar.vertical.policy: ScrollBar.AsNeeded
                TextArea {
                    id: at
                    anchors.fill: parent
                    wrapMode: TextEdit.Wrap
                    padding: 10
                    font.family: "DejaVu Sans"; font.pixelSize: 14
                    text: root.lines
                    background: null
                }
            }
            Item { Layout.fillHeight: true }
        }
    }

    Item {
        y: 140; width: 260; height: 130
        ColumnLayout {
            anchors.fill: parent
            spacing: 0
            ScrollView {
                id: b
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                ScrollBar.vertical.policy: ScrollBar.AsNeeded
                TextArea {
                    id: bt
                    anchors { left: parent.left; right: parent.right }
                    wrapMode: TextEdit.Wrap
                    padding: 24
                    font.family: "DejaVu Sans"; font.pixelSize: 14
                    text: root.lines
                    background: null
                }
            }
        }
    }

    property var views: ({ a: [a, at], b: [b, bt] })

    function r(v) { return v === undefined || v === null ? "?" : Math.round(v * 100) / 100; }
    function snap() {
        var out = [];
        for (var n in views) {
            var v = views[n][0], t = views[n][1], f = v.contentItem, o = v.ScrollBar.vertical;
            out.push(n + ": at " + r(f.contentY) + " of " + r(f.contentHeight) + " cursor " + t.cursorPosition
                + " bar hovered " + o.hovered + " active " + o.active + " position " + r(o.position)
                + " opacity " + r(o.contentItem.opacity));
        }
        return out.join("\n");
    }
    // The cursor after the last character and before the first one, as a click there puts it.
    function endA() { at.forceActiveFocus(); at.cursorPosition = at.length; }
    function homeA() { at.cursorPosition = 0; }
    function endB() { bt.forceActiveFocus(); bt.cursorPosition = bt.length; }
    function homeB() { bt.cursorPosition = 0; }
    // The view moved without its cursor, as the handle of its bar moves it.
    function downB() { b.contentItem.contentY = 88; }
}
