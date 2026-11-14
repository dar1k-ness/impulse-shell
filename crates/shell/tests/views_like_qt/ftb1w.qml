import QtQuick
import Rustshell
FloatingWindow { visible: true; implicitWidth: 300; implicitHeight: 200
Item {
    id: root
    width: 300; height: 200
    GridView {
        id: grid
        width: 200; height: 100
        model: 40
        cellWidth: 50; cellHeight: 40
        flow: GridView.FlowTopToBottom
        delegate: Rectangle { required property int index; width: 50; height: 40 }
    }
    function at(i) { const d = grid.itemAtIndex(i); return d ? d.x + "," + d.y : "none" }
    Timer { interval: 100; running: true; onTriggered: {
        console.log("cell 1 at", root.at(1), "cell 2 at", root.at(2))
        console.log("content", grid.contentWidth, grid.contentHeight)
        grid.positionViewAtIndex(30, GridView.Beginning)
        console.log("cell 30 at the beginning", grid.contentX, grid.contentY)
        console.log("done"); Qt.quit()
    } }
}
}
