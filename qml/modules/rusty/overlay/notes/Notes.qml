import QtQuick
import QtQuick.Layouts
import Rustshell
import ri.services
import ri.modules.common
import ri.modules.rusty.overlay

StyledOverlayWidget {
    id: root
    title: Translation.tr("Notes")
    showCenterButton: true

    contentItem: NotesContent {
        radius: root.contentRadius
        isClickthrough: root.clickthrough
    }
}
