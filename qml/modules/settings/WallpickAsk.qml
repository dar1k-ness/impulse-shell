import QtQuick
import QtQuick.Layouts
import ri.modules.common
import ri.modules.common.widgets
import ri.services

/**
 * What "Pick for me" asks before it picks: a style that goes by the desktop as it is now, or
 * by the user's own words (any language). The picker remembers the answer for "dislike",
 * which picks again by the same.
 */
ColumnLayout {
    id: root
    // The picker was started: the question can go away.
    signal asked()

    spacing: 8

    function byWords() {
        const words = input.text.trim();
        if (words.length === 0 || Wallpick.busy)
            return;
        Config.options.background.wallpick.request = words;
        Wallpick.pickByWords(words);
        root.asked();
    }

    StyledText {
        Layout.fillWidth: true
        wrapMode: Text.WordWrap
        text: Translation.tr("What should the new style go by?")
        color: Appearance.colors.colOnLayer2
        font.pixelSize: Appearance.font.pixelSize.small
    }
    RippleButtonWithIcon {
        Layout.fillWidth: true
        enabled: !Wallpick.busy
        materialIcon: "desktop_windows"
        mainText: Translation.tr("My desktop as it is now")
        buttonRadius: Appearance.rounding.full
        colBackground: Appearance.colors.colLayer3
        colBackgroundHover: Appearance.colors.colLayer3Hover
        onClicked: {
            Wallpick.pickByDesktop();
            root.asked();
        }
        StyledToolTip {
            text: Translation.tr("Looks for wallpapers like the one on the desktop now")
        }
    }
    StyledText {
        Layout.fillWidth: true
        wrapMode: Text.WordWrap
        text: Translation.tr("Or describe it in your words")
        color: Appearance.colors.colOnLayer2
        font.pixelSize: Appearance.font.pixelSize.small
    }
    Rectangle {
        Layout.fillWidth: true
        implicitHeight: 42
        radius: Appearance.rounding.small
        color: "transparent"
        border.width: input.activeFocus ? 2 : 1
        border.color: input.activeFocus ? Appearance.colors.colPrimary : Appearance.colors.colOutline
        clip: true

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
            text: Config.options.background.wallpick.request
            color: Appearance.colors.colOnLayer2
            Keys.onEscapePressed: input.focus = false
            onAccepted: root.byWords()
        }
        StyledText {
            anchors.fill: input
            verticalAlignment: Text.AlignVCenter
            visible: input.text.length === 0
            text: Translation.tr("night city in the rain")
            color: Appearance.colors.colSubtext
            font.pixelSize: Appearance.font.pixelSize.small
            elide: Text.ElideRight
        }
    }
    RippleButtonWithIcon {
        Layout.fillWidth: true
        enabled: !Wallpick.busy && input.text.trim().length > 0
        materialIcon: "edit"
        mainText: Translation.tr("By these words")
        buttonRadius: Appearance.rounding.full
        colBackground: Appearance.colors.colPrimaryContainer
        colBackgroundHover: Appearance.colors.colPrimaryContainerHover
        colText: Appearance.colors.colOnPrimaryContainer
        onClicked: root.byWords()
    }
}
