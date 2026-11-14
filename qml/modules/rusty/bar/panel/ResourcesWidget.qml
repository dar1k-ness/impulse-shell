import QtQuick
import ri.modules.rusty.bar as Bar
import ri.modules.rusty.verticalBar as VBar

Loader {
    property bool vertical: parent?.vertical ?? false
    property bool zoneHovered: parent?.zoneHovered ?? false
    property int shortened: parent?.shortened ?? 0
    width: parent?.width ?? implicitWidth
    height: parent?.height ?? implicitHeight
    sourceComponent: vertical ? verticalResources : horizontalResources

    Component {
        id: horizontalResources
        Bar.Resources {
            alwaysShowAllResources: shortened === 2
        }
    }
    Component {
        id: verticalResources
        VBar.Resources {}
    }
}
