// qt: 10 writes: contentY 10 contentItem.y 10 bar.position 10 gone 0 | contentItem.y=-100 contentItem.x=0 bar.position=0.063
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    property int dy: 0
    property int cy: 0
    property int ciy: 0
    property int bar: 0
    property int made: 0
    property int gone: 0
    Flickable {
        id: v
        width: 300; height: 400
        clip: true
        contentWidth: 300; contentHeight: 1600
        onContentYChanged: root.cy++
        ScrollBar.vertical: ScrollBar { id: sb; onPositionChanged: root.bar++ }
        Column {
            id: col
            Repeater {
                id: rep
                model: 40
                Rectangle {
                    width: 300; height: 40
                    onYChanged: root.dy++
                    Component.onCompleted: root.made++
                    Component.onDestruction: root.gone++
                }
            }
        }
    }
    Connections { target: v.contentItem; function onYChanged() { root.ciy++ } }
    Component.onCompleted: {
        const b = [cy, ciy, dy, bar, made, gone]
        for (let i = 1; i <= 10; i++) v.contentY = i * 10
        const a = [cy, ciy, dy, bar, made, gone]
        result = "10 writes: contentY " + (a[0]-b[0]) + " contentItem.y " + (a[1]-b[1]) + " bar.position " + (a[3]-b[3]) + " gone " + (a[5]-b[5]) + " | contentItem.y=" + v.contentItem.y + " contentItem.x=" + v.contentItem.x + " bar.position=" + sb.position.toFixed(3)
    }
}
