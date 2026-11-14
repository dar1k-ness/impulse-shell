import QtQuick
import QtQuick.Layouts

/**
 * One inner tab of a settings page; the page shows the tab whose index is its currentTab.
 */
ColumnLayout {
    required property int tabIndex
    required property string tabName
    readonly property bool settingsTab: true
    Layout.fillWidth: true
    spacing: 24
}
