// OptionsToolbar.qml of the original reduced to its mode tabs, as the original wrote the handler.
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
            root.selectionMode = currentIndex === 0 ? 0 : 1
        }
    }
}
