import QtQuick
Item {
    id: root
    width: 200; height: 200
    ListView {
        id: f
        width: 200; height: 200
        model: 40
        highlightRangeMode: ListView.StrictlyEnforceRange; preferredHighlightBegin: 60; preferredHighlightEnd: 90
        delegate: Rectangle { required property int index; width: 200; height: 30 }
        Component.onCompleted: console.log("completed", contentY, contentHeight, count, currentIndex, currentItem !== null)
    }
    Timer { interval: 100; running: true; onTriggered: {
        console.log("later", f.contentY, f.contentHeight, f.count, f.currentIndex, f.currentItem !== null)
        console.log("done")
    } }
}
