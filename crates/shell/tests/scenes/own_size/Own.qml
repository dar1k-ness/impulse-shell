import QtQuick

// A view's height from the height of its content item, as WaffleNotificationPopup.qml has it:
// the binding reads the view's own size on the way and is no loop.
Item {
    id: root
    width: 300
    height: 400
    property string result: [v.height, v.contentItem.height, v.contentHeight, w.width, w.contentItem.width].join(" ")

    ListView {
        id: v
        topMargin: 16
        bottomMargin: 16
        height: Math.min(contentItem.height + topMargin + bottomMargin, parent.height)
        width: parent.width
        model: 2
        delegate: Item {
            width: 10
            height: 20
        }
    }
    ListView {
        id: w
        orientation: ListView.Horizontal
        leftMargin: 5
        rightMargin: 5
        width: Math.min(contentItem.width + leftMargin + rightMargin, parent.width)
        height: 10
        model: 3
        delegate: Item {
            width: 30
            height: 10
        }
    }
}
