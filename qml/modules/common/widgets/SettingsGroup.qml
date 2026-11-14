import QtQuick
import QtQuick.Layouts
import ri.modules.common
import ri.modules.common.widgets

/**
 * A titled group of settings rows. Rows inside are stacked with a thin gap; the first and last
 * shown row round the group's outer corners.
 */
ColumnLayout {
    id: root
    property string title: ""
    property bool shown: true
    readonly property bool settingsGroup: true
    default property alias rowsData: rows.data

    visible: shown
    Layout.fillWidth: true
    spacing: 8

    StyledText {
        visible: root.title.length > 0
        Layout.leftMargin: 16
        text: root.title
        color: Appearance.colors.colPrimary
        font.pixelSize: Appearance.font.pixelSize.small
        font.weight: Font.Medium
    }

    ColumnLayout {
        id: rows
        Layout.fillWidth: true
        spacing: 2
    }
}
