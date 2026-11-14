import QtQuick
import QtQuick.Controls
Button {
    property var log
    property string tag
    MouseArea { objectName: "ma" }
    contentItem: Item {
        objectName: "inner"
        Component.onCompleted: if (log) log.push(tag + "-inner")
    }
}
