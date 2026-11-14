// The same as scrollbar_sizes.qml for an attached horizontal ScrollBar
// (QQuickScrollBarAttachedPrivate::layoutHorizontal: the Flickable's width and a place at its
// bottom edge), and for a Flickable with both bars.
import QtQuick
import QtQuick.Controls
Item {
    id: scene
    width: 700; height: 240
    property int frames: 4
    property string log: ""
    Flickable {
        id: h0; x: 0; width: 100; height: 50
        contentWidth: 300; contentHeight: 50
        ScrollBar.horizontal: ScrollBar { id: h0bar; contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
    }
    Flickable {
        id: h1; x: 110; width: 100; height: 50
        contentWidth: 300; contentHeight: 50
        ScrollBar.horizontal: ScrollBar { id: h1bar; width: 60; contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
    }
    Flickable {
        id: h2; x: 220; width: 100; height: 50
        contentWidth: 300; contentHeight: 50
        ScrollBar.horizontal: ScrollBar { id: h2bar; height: 9; contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
    }
    Flickable {
        id: h3; x: 330; width: 100; height: 50
        contentWidth: 300; contentHeight: 50
        ScrollBar.horizontal: ScrollBar { id: h3bar; x: 10; width: parent ? parent.width - 20 : 0; contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
    }
    Flickable {
        id: h4; x: 440; width: 100; height: 50
        contentWidth: 300; contentHeight: 50
        ScrollBar.horizontal: ScrollBar { id: h4bar; y: 3; contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
    }
    Flickable {
        id: h5; x: 550; width: 100; height: 50
        contentWidth: 300; contentHeight: 50
        ScrollBar.horizontal: ScrollBar { id: h5bar; x: 10; width: 60; contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
    }
    Flickable {
        id: b0; y: 60; width: 100; height: 50
        contentWidth: 300; contentHeight: 200
        ScrollBar.vertical: ScrollBar { id: b0v; contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
        ScrollBar.horizontal: ScrollBar { id: b0h; contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
    }
    ScrollView {
        id: s1; x: 110; y: 120; width: 100; height: 80
        contentWidth: 300; contentHeight: 300
        Item { width: 300; height: 300 }
        ScrollBar.horizontal: ScrollBar { id: s1bar; width: 70; contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
    }
    ScrollView {
        id: s2; x: 220; y: 120; width: 100; height: 80
        contentWidth: 300; contentHeight: 300
        Item { width: 300; height: 300 }
        ScrollBar.horizontal: ScrollBar { id: s2bar; parent: s2; y: s2.height - height; width: 70; contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
    }
    ScrollView {
        id: s3; x: 330; y: 120; width: 100; height: 80
        contentWidth: 300; contentHeight: 300
        Item { width: 300; height: 300 }
        ScrollBar.horizontal: ScrollBar { id: s3bar; anchors.bottom: parent.bottom; anchors.left: parent.left; anchors.right: parent.right; padding: 5; contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
    }
    function sample(frame) {
        if (frame === 2)
            for (const f of [h0, h1, h2, h3, h4, h5, b0, s1, s2, s3]) {
                f.width += 10
                f.height += 20
            }
    }
    function shown(frame) {
        const bars = { h0: h0bar, h1: h1bar, h2: h2bar, h3: h3bar, h4: h4bar, h5: h5bar, b0v: b0v, b0h: b0h, s1: s1bar, s2: s2bar, s3: s3bar }
        for (const k in bars) {
            const b = bars[k]
            log += frame + " " + k + " " + [b.x, b.y, b.width, b.height].map(v => +v.toFixed(2)).join(" ") + "\n"
        }
    }
}
