import QtQuick
import QtQuick.Controls

// The handle of the bar of an editor of qml/ (AssistantChat.qml, NotesContent.qml) under the
// pointer. The Flickable a ScrollView makes is `pixelAligned`: the handle moves its content to
// a whole pixel. While the handle is held the bar stands where the content is
// (QQuickFlickableVisibleArea::updateVisible); let go, it stands where the pointer put it
// (QQuickScrollBarPrivate::handleRelease), and the content stays.
Item {
    id: root
    width: 260; height: 120

    readonly property string lines: "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\neleven\ntwelve\nthirteen\nfourteen\nfifteen\nsixteen"

    function snap() {
        const r = v => Math.round(v * 1000000) / 1000000;
        const f = a.contentItem, o = a.ScrollBar.vertical;
        return "at " + r(f.contentY) + " of " + r(f.contentHeight)
            + " pressed " + o.pressed + " position " + r(o.position) + " size " + r(o.size)
            + " handle " + r(o.contentItem.y) + " " + r(o.contentItem.height);
    }

    ScrollView {
        id: a
        width: 260; height: 100
        clip: true
        TextArea {
            anchors.fill: parent
            wrapMode: TextEdit.Wrap
            padding: 10
            font.family: "DejaVu Sans"; font.pixelSize: 14
            text: root.lines
            background: null
        }
    }
}
