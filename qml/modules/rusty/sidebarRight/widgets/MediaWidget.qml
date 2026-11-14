import QtQuick
import ri.modules.common
import ri.modules.rusty.mediaControls

// Sidebar widget "media": the original's media controls for the active player on its own card.
// In a row of toggles it is as high as they are: the cover with the title beside it, or the
// cover alone in a single cell.
SidebarCard {
    id: root
    property real padding: root.inGrid ? 0 : 8
    color: root.inGrid ? "transparent" : Appearance.colors.colLayer1
    implicitWidth: root.inGrid ? root.cellsWidth : 0
    implicitHeight: root.inGrid ? root.baseCellHeight : media.implicitHeight + root.padding * 2

    MediaCard {
        id: media
        anchors.fill: parent
        anchors.margins: root.padding
        radius: root.radius - root.padding
        placeholderColor: root.inGrid ? Appearance.colors.colLayer2 : "transparent"
    }
}
