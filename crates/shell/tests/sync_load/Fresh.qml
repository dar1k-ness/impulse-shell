import QtQuick
Item {
    property string tag: "other"
    Component.onCompleted: if (parent && parent.log) parent.log.push("completed tag=" + tag)
}
