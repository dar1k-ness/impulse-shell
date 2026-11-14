import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The three shapes of ScrollView in qml/: the input of AssistantChat.qml (`a`), the editor of
// NotesContent.qml (`b`) and the code of MessageCodeBlock.qml (`c`). `snap()` gives the
// geometry of each view, of the Flickable it makes, of the field and of both attached bars.
Item {
    id: root
    width: 260; height: 420

    readonly property string lines: "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten"

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

    Item {
        y: 280; width: 260; height: 130
        ColumnLayout {
            anchors.left: parent.left; anchors.right: parent.right
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
                    anchors.bottom: parent.bottom
                    anchors.left: parent.left
                    anchors.right: parent.right
                    padding: 5
                    policy: ScrollBar.AsNeeded
                    opacity: visualSize == 1 ? 0 : 1
                    visible: opacity > 0
                    contentItem: Rectangle {
                        implicitHeight: 6
                        radius: 4
                        color: "#808080"
                    }
                }
                TextArea {
                    id: ct
                    Layout.fillWidth: true
                    readOnly: true
                    font.family: "DejaVu Sans Mono"; font.pixelSize: 13
                    text: "fn main() { println!(\"a rather long line of code that does not fit\"); }\nlet x = 1;\nlet y = 2;"
                    background: null
                }
            }
        }
    }

    property var views: ({ a: [a, at], b: [b, bt], c: [c, ct] })

    function r(v) { return v === undefined || v === null ? "?" : Math.round(v * 100) / 100; }
    function box(o) { return o ? [r(o.x), r(o.y), r(o.width), r(o.height)].join(" ") : "none"; }
    function bar(n, v, o) {
        if (!o)
            return [n + ": none", n + " size none", n + " content: none"];
        return [
            n + ": " + box(o) + " in view " + (o.parent === v) + " orientation " + o.orientation
                + " visible " + o.visible + " policy " + o.policy,
            n + " size " + r(o.size) + " position " + r(o.position) + " visualSize " + r(o.visualSize)
                + " visualPosition " + r(o.visualPosition),
            n + " content: " + box(o.contentItem)
        ];
    }
    function snap() {
        var out = [];
        for (var n in views) {
            var v = views[n][0], t = views[n][1], f = v.contentItem;
            out.push(n + " view: " + box(v) + " content " + r(v.contentWidth) + " " + r(v.contentHeight)
                + " available " + r(v.availableWidth) + " " + r(v.availableHeight)
                + " implicit " + r(v.implicitWidth) + " " + r(v.implicitHeight));
            out.push(n + " flickable: " + (f ? box(f) + " content " + r(f.contentWidth) + " " + r(f.contentHeight)
                + " at " + r(f.contentX) + " " + r(f.contentY) + " clip " + f.clip + " in view " + (f.parent === v) : "none"));
            out.push(n + " contentItem: " + box(f ? f.contentItem : null));
            out.push(n + " field: " + box(t) + " implicit " + r(t.implicitWidth) + " " + r(t.implicitHeight)
                + " in contentItem " + (f ? t.parent === f.contentItem : false));
            out = out.concat(bar(n + " vertical", v, v.ScrollBar.vertical));
            out = out.concat(bar(n + " horizontal", v, v.ScrollBar.horizontal));
        }
        return out.join("\n");
    }
    // Scrolled to the end, as a wheel does through the Flickable.
    function end() {
        for (var n in views) {
            var f = views[n][0].contentItem;
            if (!f)
                continue;
            f.contentX = Math.max(0, f.contentWidth - f.width);
            f.contentY = Math.max(0, f.contentHeight - f.height);
        }
    }
}
