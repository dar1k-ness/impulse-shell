import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// A message of the assistant chat (AssistantMessage.qml): a paragraph that wraps
// (MessageTextBlock.qml) above the code of MessageCodeBlock.qml, each a ColumnLayout made by a
// Repeater in the ColumnLayout of the message, which is a delegate of a ListView. `both()`,
// `codeOnly()` and `textOnly()` add a message that has its blocks as it is made, `empty()`
// one that gets them from `fill()`; `narrow()` and `wide()` resize the list. `snap()` gives
// the view of every code block, its Flickable, its TextArea and its horizontal bar.
Item {
    id: root
    width: 420; height: 600
    property string para: "This is a rather long paragraph that comes before the code block, so that the text has to wrap over several lines of the sidebar: it talks about tab characters in Python sources, how they are drawn in a monospace font, and why the width of a tab stop matters when two shells are compared pixel by pixel."
    property string code: "def f(x):\n\tif x:\n\t\treturn 1\n\treturn 2"
    property int msgs: 0
    property var blocks0: []
    function both() { blocks0 = [{type: "text", content: para}, {type: "code", content: code}]; msgs = 1 }
    function codeOnly() { blocks0 = [{type: "code", content: code}]; msgs = 1 }
    function fill() { blocks0 = [{type: "text", content: para}, {type: "code", content: code}] }
    function textOnly() { blocks0 = [{type: "text", content: para}]; msgs = 1 }
    function empty() { msgs = 1 }
    function narrow() { list.anchors.rightMargin = 130 }
    function wide() { list.anchors.rightMargin = 10 }

    ListView {
        id: list
        anchors.fill: parent
        anchors.margins: 10
        spacing: 10
        model: root.msgs
        delegate: Rectangle {
            id: msg
            property list<var> blocks: root.blocks0
            anchors.left: parent?.left
            anchors.right: parent?.right
            implicitHeight: col.implicitHeight + 14
            color: "#303030"
            ColumnLayout {
                id: col
                anchors { left: parent.left; right: parent.right; top: parent.top; margins: 7 }
                spacing: 3
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    Repeater {
                        model: msg.blocks
                        delegate: DelegateChooser {
                            role: "type"
                            DelegateChoice { roleValue: "text"; ColumnLayout {
                                required property var modelData
                                Layout.fillWidth: true
                                spacing: 0
                                TextArea {
                                    Layout.fillWidth: true
                                    readOnly: true
                                    wrapMode: TextEdit.Wrap
                                    font.family: "DejaVu Sans"; font.pixelSize: 13
                                    text: modelData.content
                                    background: null
                                }
                            } }
                            DelegateChoice { roleValue: "code"; ColumnLayout {
                                id: cb
                                required property var modelData
                                spacing: 2
                                Rectangle { Layout.fillWidth: true; implicitHeight: 30; color: "#404040" }
                                RowLayout {
                                    spacing: 2
                                    Rectangle { implicitWidth: 40; Layout.fillHeight: true; Layout.fillWidth: false; color: "#404040" }
                                    Rectangle {
                                        Layout.fillWidth: true
                                        color: "#404040"
                                        implicitHeight: ccl.implicitHeight
                                        ColumnLayout {
                                            id: ccl
                                            anchors.fill: parent
                                            spacing: 0
                                            ScrollView {
                                                id: c
                                                Layout.fillWidth: true
                                                implicitWidth: parent.width
                                                implicitHeight: ct.implicitHeight + 1
                                                contentWidth: ct.width - 1
                                                clip: true
                                                ScrollBar.vertical.policy: ScrollBar.AlwaysOff
                                                ScrollBar.horizontal: ScrollBar {
                                                    id: hb
                                                    anchors.bottom: parent.bottom
                                                    anchors.left: parent.left
                                                    anchors.right: parent.right
                                                    padding: 5
                                                    policy: ScrollBar.AsNeeded
                                                    opacity: visualSize == 1 ? 0 : 1
                                                    visible: opacity > 0
                                                    contentItem: Rectangle { implicitHeight: 6; radius: 4; color: "#808080" }
                                                }
                                                TextArea {
                                                    id: ct
                                                    Layout.fillWidth: true
                                                    readOnly: true
                                                    font.family: "DejaVu Sans Mono"; font.pixelSize: 13
                                                    text: cb.modelData.content
                                                    background: null
                                                }
                                                Component.onCompleted: root.views.push([c, ct, hb])
                                            }
                                        }
                                    }
                                }
                            } }
                        }
                    }
                }
            }
        }
    }
    property var views: []
    function r(v) { return v === undefined || v === null ? "?" : Math.round(v * 100) / 100; }
    function snap() {
        var out = [];
        for (var i = 0; i < views.length; i++) {
            var v = views[i][0], t = views[i][1], b = views[i][2], f = v.contentItem;
            out.push("view " + r(v.width) + "x" + r(v.height) + " implicit " + r(v.implicitWidth) + " contentWidth " + r(v.contentWidth)
                + " flick " + r(f.width) + " content " + r(f.contentWidth) + " area " + r(t.width) + " implicit " + r(t.implicitWidth)
                + " bar size " + r(b.size) + " visualSize " + r(b.visualSize) + " visible " + b.visible);
        }
        return out.join("\n");
    }
}
