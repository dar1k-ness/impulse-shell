import ri.modules.common
import ri.modules.common.widgets
import ri.services
import ri
import ri.modules.common.functions

import QtQuick
import QtQuick.Layouts
import Rustshell.Services.Mpris
import Rustshell.Hyprland

Item {
    id: root
    property bool borderless: Config.options.bar.borderless
    readonly property MprisPlayer activePlayer: MprisController.activePlayer
    // Off where the bar gives it no room for the title: the progress ring alone.
    property bool showTitle: true
    readonly property string cleanedTitle: StringUtils.cleanMusicTitle(activePlayer?.trackTitle) || Translation.tr("No media")
    // The least length its title still reads at: the progress ring and the title, a long one
    // cut with an ellipsis. 0 where it shows no title. Measured apart from the title itself,
    // which is hidden where it has no room.
    readonly property real titledLength: Config.options.bar.verbose ? mediaCircProg.implicitSize + Math.min(Math.ceil(titleLength.width), 120) + rowLayout.spacing * 4 : 0
    // Room kept clear at each end, where edit mode's handles are on a player of a set length.
    property real endRoom: 0

    TextMetrics {
        id: titleLength
        font: title.font
        text: title.text
    }

    Layout.fillHeight: true
    implicitWidth: rowLayout.implicitWidth + rowLayout.spacing * 2
    implicitHeight: Appearance.sizes.barHeight

    // MPRIS does not signal a position that merely advances. MediaControls groups duplicate
    // players by their positions, so this ticks while the controls are open as well.
    UpdateTimer {
        running: activePlayer?.playbackState == MprisPlaybackState.Playing && (root.visible || GlobalStates.mediaControlsOpen)
        onTriggered: activePlayer.positionChanged()
    }

    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.MiddleButton | Qt.BackButton | Qt.ForwardButton | Qt.RightButton | Qt.LeftButton
        onPressed: (event) => {
            if (event.button === Qt.MiddleButton) {
                activePlayer.togglePlaying();
            } else if (event.button === Qt.BackButton) {
                activePlayer.previous();
            } else if (event.button === Qt.ForwardButton || event.button === Qt.RightButton) {
                activePlayer.next();
            } else if (event.button === Qt.LeftButton) {
                GlobalStates.mediaControlsOpen = !GlobalStates.mediaControlsOpen
            }
        }
    }

    RowLayout { // Real content
        id: rowLayout

        spacing: 4
        anchors.fill: parent
        anchors.leftMargin: root.endRoom
        anchors.rightMargin: root.endRoom

        ClippedFilledCircularProgress {
            id: mediaCircProg
            Layout.alignment: Qt.AlignVCenter
            lineWidth: Appearance.rounding.unsharpen
            value: activePlayer?.position / activePlayer?.length
            implicitSize: 20
            colPrimary: Appearance.colors.colOnSecondaryContainer
            enableAnimation: false

            Item {
                anchors.centerIn: parent
                width: mediaCircProg.implicitSize
                height: mediaCircProg.implicitSize
                
                MaterialSymbol {
                    anchors.centerIn: parent
                    fill: 1
                    text: activePlayer?.isPlaying ? "pause" : "music_note"
                    iconSize: Appearance.font.pixelSize.normal
                    color: Appearance.m3colors.m3onSecondaryContainer
                }
            }
        }

        StyledText {
            id: title
            visible: Config.options.bar.verbose && root.showTitle
            width: rowLayout.width - (CircularProgress.size + rowLayout.spacing * 2)
            Layout.alignment: Qt.AlignVCenter
            Layout.fillWidth: true // Ensures the text takes up available space
            Layout.rightMargin: rowLayout.spacing
            horizontalAlignment: Text.AlignHCenter
            elide: Text.ElideRight // Truncates the text on the right
            color: Appearance.colors.colOnLayer1
            text: `${cleanedTitle}${activePlayer?.trackArtist ? ' • ' + activePlayer.trackArtist : ''}`
        }

    }

}
