// OptionsToolbar.qml of the original reduced to its mode tabs, with the handler of this tree.
import QtQuick

Item {
    id: root
    property var action
    property var selectionMode
    property alias currentIndex: tabs.currentIndex
    function press(i) { tabs.press(i) }
    Tabs {
        id: tabs
        tabButtonList: [1, 2]
        currentIndex: root.selectionMode === 0 ? 0 : 1
        onCurrentIndexChanged: {
            if (root.selectionMode === undefined)
                return;
            root.selectionMode = currentIndex === 0 ? 0 : 1
        }
    }
}
