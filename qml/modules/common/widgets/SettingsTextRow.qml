import QtQuick
import QtQuick.Layouts
import ri.modules.common
import ri.modules.common.widgets

/**
 * A row with a one-line text field. Bind `text`, write it back in `onEdited`.
 */
SettingsRow {
    id: root
    property string text: ""
    property string placeholderText: ""
    property real fieldWidth: 300
    signal edited(string text)

    Rectangle {
        Layout.preferredWidth: root.fieldWidth
        implicitHeight: 42
        radius: Appearance.rounding.small
        color: "transparent"
        border.width: input.activeFocus ? 2 : 1
        border.color: input.activeFocus ? Appearance.colors.colPrimary : Appearance.colors.colOutline
        clip: true

        // The whole field focuses the input on the first click
        MouseArea {
            anchors.fill: parent
            cursorShape: Qt.IBeamCursor
            onPressed: input.forceActiveFocus()
        }

        StyledTextInput {
            id: input
            anchors {
                left: parent.left
                right: parent.right
                verticalCenter: parent.verticalCenter
                leftMargin: 14
                rightMargin: 14
            }
            clip: true
            text: root.text
            Keys.onEscapePressed: input.focus = false
            onAccepted: input.focus = false
            color: Appearance.colors.colOnLayer2
            onTextChanged: {
                if (text !== root.text)
                    root.edited(text);
            }
        }
        StyledText {
            anchors.fill: input
            verticalAlignment: Text.AlignVCenter
            visible: input.text.length === 0
            text: root.placeholderText
            color: Appearance.colors.colSubtext
            font.pixelSize: Appearance.font.pixelSize.small
            elide: Text.ElideRight
        }
    }
}
