import QtQuick
import Quickshell
import Quickshell.Services.Notifications

// the original's `Notifications.discardAllNotifications` (qml/services/Notifications.qml) in small: a
// wrapper per notification, and "clear all" that dismisses the server's notifications with
// `forEach` over the list they leave, then destroys the wrappers.
ShellRoot {
    id: root

    property list<QtObject> list

    component Notif: QtObject {
        required property int notificationId
        property Notification notification
        property string summary: notification?.summary ?? ""

        onSummaryChanged: {
            if (summary === "")
                console.log("the summary of " + notificationId + " is empty");
        }
    }

    Component {
        id: notifComponent

        Notif {}
    }

    NotificationServer {
        id: server

        onNotification: (notification) => {
            notification.tracked = true;
            if (notification.summary === "mark") {
                notification.closed.connect(() => Qt.quit());
                return;
            }
            root.list.push(notifComponent.createObject(root, {
                "notificationId": notification.id,
                "notification": notification
            }));
            if (notification.summary === "third")
                Qt.callLater(root.clearAll);
        }
    }

    function clearAll() {
        const discarded = root.list.slice(0);
        root.list = [];
        server.trackedNotifications.values.forEach((notif) => {
            notif.dismiss();
        });
        discarded.forEach((notifObject) => notifObject.destroy());
        console.log("cleared");
    }
}
