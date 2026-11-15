import ri
import ri.modules.common
import ri.modules.common.widgets
import ri.services
import Qt5Compat.GraphicalEffects
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root

    // The sidebar opens with the groups folded, at the first one (docs/original-differences.md).
    Connections {
        target: GlobalStates
        function onSidebarRightOpenChanged() {
            listview.collapseGroups();
            listview.scrollToStart();
        }
    }

    // A sidebar that is not shown makes the rows of its notifications when something asks for
    // them: the click that opens it, which then waits for all of them. A closed sidebar asks a
    // moment after it is made and after its notifications change instead, so opening it has
    // none left to make.
    Connections {
        target: Notifications
        function onListChanged() {
            if (!GlobalStates.sidebarRightOpen)
                fillWhileClosed.restart();
        }
    }
    Timer {
        id: fillWhileClosed
        interval: 1000
        running: true
        onTriggered: {
            if (!GlobalStates.sidebarRightOpen)
                listview.fillRows();
        }
    }

    NotificationListView { // Scrollable window
        id: listview
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: statusRow.top
        anchors.bottomMargin: 5

        clip: true
        layer.enabled: true
        layer.effect: OpacityMask {
            maskSource: Rectangle {
                width: listview.width
                height: listview.height
                radius: Appearance.rounding.normal
            }
        }

        popup: false
    }

    // Placeholder when list is empty
    PagePlaceholder {
        shown: Notifications.list.length === 0
        icon: "notifications_active"
        description: Translation.tr("Nothing")
        shape: MaterialShape.Shape.Ghostish
        descriptionHorizontalAlignment: Text.AlignHCenter
    }

    ButtonGroup {
        id: statusRow
        anchors {
            left: parent.left
            right: parent.right
            bottom: parent.bottom
        }

        NotificationStatusButton {
            Layout.fillWidth: false
            buttonIcon: "notifications_paused"
            toggled: Notifications.silent
            onClicked: () => {
                Notifications.silent = !Notifications.silent;
            }
        }
        NotificationStatusButton {
            enabled: false
            Layout.fillWidth: true
            buttonText: Translation.tr("%1 notifications").arg(Notifications.list.length)
        }
        NotificationStatusButton {
            Layout.fillWidth: false
            buttonIcon: "delete_sweep"
            onClicked: () => {
                Notifications.discardAllNotifications()
            }
        }
    }
}