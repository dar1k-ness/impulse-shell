// The geometry of an attached vertical ScrollBar whose width, height or position the scene
// sets. A bare Flickable lays its bar out (QQuickScrollBarAttachedPrivate::layoutVertical): the
// bar gets the Flickable's height and a place at its right edge whatever the scene set for
// those, and keeps the scene's width and y. A ScrollView leaves a bar it is given alone.
// Frame 2 resizes every Flickable and ScrollView; after each frame the log gets every bar's x,
// y, width and height.
import QtQuick
import QtQuick.Controls
Item {
    id: scene
    width: 700; height: 240
    property int frames: 4
    property string log: ""
    ListView {
        id: v0; x: 0; width: 60; height: 100
        model: 20
        delegate: Item { width: 60; height: 20 }
        ScrollBar.vertical: ScrollBar { id: v0bar; contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
    }
    ListView {
        id: v1; x: 70; width: 60; height: 100
        model: 20
        delegate: Item { width: 60; height: 20 }
        ScrollBar.vertical: ScrollBar { id: v1bar; height: 40; contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
    }
    ListView {
        id: v2; x: 140; width: 60; height: 100
        model: 20
        delegate: Item { width: 60; height: 20 }
        ScrollBar.vertical: ScrollBar { id: v2bar; width: 12; contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
    }
    ListView {
        id: v3; x: 210; width: 60; height: 100
        model: 20
        delegate: Item { width: 60; height: 20 }
        ScrollBar.vertical: ScrollBar { id: v3bar; y: 10; height: parent ? parent.height - 20 : 0; contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
    }
    ListView {
        id: v4; x: 280; width: 60; height: 100
        model: 20
        delegate: Item { width: 60; height: 20 }
        ScrollBar.vertical: ScrollBar { id: v4bar; x: 3; contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
    }
    ListView {
        id: v5; x: 350; width: 60; height: 100
        model: 20
        delegate: Item { width: 60; height: 20 }
        ScrollBar.vertical: ScrollBar { id: v5bar; y: 10; height: 40; contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
    }
    ScrollView {
        id: s1; x: 0; y: 120; width: 100; height: 80
        contentWidth: 300; contentHeight: 300
        Item { width: 300; height: 300 }
        ScrollBar.vertical: ScrollBar { id: s1bar; height: 50; contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
    }
    ScrollView {
        id: s2; x: 110; y: 120; width: 100; height: 80
        contentWidth: 300; contentHeight: 300
        Item { width: 300; height: 300 }
        ScrollBar.vertical: ScrollBar { id: s2bar; parent: s2; x: s2.width - width; height: s2.height; contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
    }
    ScrollView {
        id: s3; x: 220; y: 120; width: 100; height: 80
        contentWidth: 300; contentHeight: 300
        Item { width: 300; height: 300 }
        ScrollBar.vertical: ScrollBar { id: s3bar; anchors.top: parent.top; anchors.bottom: parent.bottom; anchors.right: parent.right; padding: 5; contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
    }
    function sample(frame) {
        if (frame === 2)
            for (const f of [v0, v1, v2, v3, v4, v5, s1, s2, s3]) {
                f.width += 10
                f.height += 20
            }
    }
    function shown(frame) {
        const bars = { v0: v0bar, v1: v1bar, v2: v2bar, v3: v3bar, v4: v4bar, v5: v5bar, s1: s1bar, s2: s2bar, s3: s3bar }
        for (const k in bars) {
            const b = bars[k]
            log += frame + " " + k + " " + [b.x, b.y, b.width, b.height].map(v => +v.toFixed(2)).join(" ") + "\n"
        }
    }
}
