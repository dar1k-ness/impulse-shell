import QtQuick
import Rustshell
// A small copy of qml/modules/rusty/wallpaperSelector/WallpaperSelectorContent.qml:290-308.
FloatingWindow { visible: true; implicitWidth: 200; implicitHeight: 100
Item {
    id: root
    width: 200; height: 100
    property string log: ""
    property string result: log + " end cur " + grid.currentIndex + " y " + grid.contentY + " n " + grid.count
    ListModel { id: lm }
    GridView {
        id: grid
        objectName: "grid"
        readonly property int columns: 4
        property int currentIndex: 0
        anchors.fill: parent
        cellWidth: width / 4
        cellHeight: 40
        interactive: true
        clip: true
        keyNavigationWraps: true
        boundsBehavior: Flickable.StopAtBounds
        function moveSelection(delta) {
            currentIndex = Math.max(0, Math.min(grid.model.count - 1, currentIndex + delta));
            positionViewAtIndex(currentIndex, GridView.Contain);
        }
        model: lm
        onModelChanged: currentIndex = 0
        onCurrentIndexChanged: root.log += " c" + currentIndex
        delegate: Rectangle {
            required property int index
            objectName: index < 10 ? "d0" + index : "d" + index
            width: grid.cellWidth; height: grid.cellHeight
            color: index === grid.currentIndex ? "#ff0000" : "#404040"
            border.width: 1; border.color: "#000000"
        }
    }
    Component.onCompleted: { for (let i = 0; i < 40; i++) lm.append({filePath: "f" + i}) }
    Timer { interval: 100; running: true; onTriggered: { grid.moveSelection(13); root.log += " move y " + grid.contentY } }
    Timer { interval: 300; running: true; onTriggered: { grid.currentIndex = 4; root.log += " hover y " + grid.contentY } }
    Timer { interval: 500; running: true; onTriggered: { grid.currentIndex = 30; root.log += " far y " + grid.contentY } }
    Timer { interval: 700; running: true; onTriggered: { console.log(root.result); console.log("done"); Qt.quit() } }
}
}
