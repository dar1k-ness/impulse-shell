import QtQuick

Item {
    property bool vertical: parent?.vertical ?? false
    property bool zoneHovered: parent?.zoneHovered ?? false
    property int shortened: parent?.shortened ?? 0
    width: parent?.width ?? implicitWidth
    height: parent?.height ?? implicitHeight
}
