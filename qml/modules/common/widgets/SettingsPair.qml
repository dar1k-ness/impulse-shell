import QtQuick
import QtQuick.Layouts

/**
 * Two settings rows side by side inside a SettingsGroup.
 */
RowLayout {
    property bool shown: true
    readonly property bool settingsPair: true
    visible: shown
    Layout.fillWidth: true
    spacing: 2
    uniformCellSizes: true
}
