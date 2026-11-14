import QtQuick
import QtQuick.Layouts
import ri.modules.common
import ri.modules.common.widgets

/**
 * Scrollable settings page: a centered column of SettingsGroups.
 */
StyledFlickable {
    id: root
    property real columnWidth: 860
    property real bottomContentPadding: 40
    readonly property bool settingsPage: true
    default property alias contentData: contentColumn.data

    clip: true
    contentHeight: contentColumn.implicitHeight + root.bottomContentPadding

    // The item scrollToItem keeps in view while the layout settles (a page or tab switch, rows
    // revealing above it) until the user scrolls
    property Item pinnedItem: null
    property real pinnedY: 0

    function scrollToItem(item) {
        root.pinnedItem = item;
        root.followPinned();
    }

    function followPinned() {
        if (!root.pinnedItem)
            return;
        const p = root.pinnedItem.mapToItem(contentColumn, 0, 0);
        const maxY = Math.max(0, root.contentHeight - root.height);
        root.pinnedY = Math.max(0, Math.min(maxY, p.y - 24));
        root.scrollTargetY = root.pinnedY;
        root.contentY = root.pinnedY;
    }

    onContentHeightChanged: root.followPinned()
    onHeightChanged: root.followPinned()
    onMovementStarted: root.pinnedItem = null
    onScrollTargetYChanged: if (root.scrollTargetY !== root.pinnedY) root.pinnedItem = null

    // Clicking empty space takes focus away from a text field
    MouseArea {
        width: root.width
        height: Math.max(root.height, root.contentHeight)
        onPressed: root.forceActiveFocus()
    }

    ColumnLayout {
        id: contentColumn
        width: Math.min(root.columnWidth, root.width - 64)
        anchors {
            top: parent.top
            topMargin: 4
            horizontalCenter: parent.horizontalCenter
        }
        spacing: 24
    }
}
