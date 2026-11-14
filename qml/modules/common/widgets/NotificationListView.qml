pragma ComponentBehavior: Bound

import ri.modules.common.widgets
import ri.services
import QtQuick
import Rustshell

StyledListView { // Scrollable window
    id: root
    property bool popup: false
    // Every expanded group folds.
    signal collapseGroups()

    spacing: 3

    model: ScriptModel {
        values: root.popup ? Notifications.popupAppNameList : Notifications.appNameList
    }
    delegate: NotificationGroup {
        id: group
        required property int index
        required property var modelData
        popup: root.popup
        width: ListView.view.width // https://doc.qt.io/qt-6/qml-qtquick-listview.html
        notificationGroup: popup ? 
            Notifications.popupGroupsByAppName[modelData] :
            Notifications.groupsByAppName[modelData]
        Connections {
            target: root
            function onCollapseGroups() {
                if (group.expanded)
                    group.toggleExpanded();
            }
        }
    }
}
