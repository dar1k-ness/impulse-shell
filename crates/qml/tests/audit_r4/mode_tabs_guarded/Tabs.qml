// ToolbarTabBar.qml reduced: `currentIndex` is an alias of the TabBar's.
import QtQuick
import QtQuick.Controls

Item {
    id: root
    property alias currentIndex: tabBar.currentIndex
    required property var tabButtonList
    // What a press on tab `i` does.
    function press(i) { buttons.itemAt(i).click() }
    TabBar {
        id: tabBar
        Repeater { id: buttons; model: root.tabButtonList.length; delegate: TabButton {} }
    }
}
