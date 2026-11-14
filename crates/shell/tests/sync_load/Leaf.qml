import QtQuick
Item {
    property string tag: "leaf"
    onTagChanged: if (parent && parent.log) parent.log.push("tag=" + tag)
    Component.onCompleted: if (parent && parent.log) parent.log.push("completed tag=" + tag + " status=" + parent.status + " item=" + (parent.item === this))
}
