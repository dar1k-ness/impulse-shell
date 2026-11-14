import QtQuick
import ri.modules.common
import ri.modules.common.widgets

/**
 * A row with a switch; clicking anywhere on the row toggles it.
 * Bind `checked` to the option and write it back in `onUserToggled`.
 */
SettingsRow {
    id: root
    property bool checked: false
    signal userToggled(bool value)

    clickable: true
    onClicked: root.userToggled(!root.checked)

    StyledSwitch {
        scale: 0.85
        checked: root.checked
        enabled: root.enabled
        // The row owns the click so the switch never breaks its binding to `checked`
        MouseArea {
            anchors.fill: parent
            cursorShape: Qt.PointingHandCursor
            onClicked: root.userToggled(!root.checked)
        }
    }
}
