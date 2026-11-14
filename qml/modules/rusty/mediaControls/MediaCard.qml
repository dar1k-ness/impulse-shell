pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import ri.services
import ri.modules.common
import ri.modules.common.widgets

/**
 * The active MPRIS player as the original's media controls draw it (PlayerControl: blurred cover art,
 * art-tinted colors, wavy seek bar), filling this item edge to edge; with no player, the original's
 * "No active player" placeholder on a card of `placeholderColor`.
 *
 * The card takes the form its size has room for (`form`): at the original's size and wider the
 * original's layout ("full", its text and buttons larger in a larger card); the cover above the
 * controls ("tall") in a narrow high card; the cover with the title beside it ("row") in a low
 * one; and the cover alone, a circle ("circle"), where no text fits.
 */
Item {
    id: root
    property real radius: Appearance.rounding.normal
    property color placeholderColor: Appearance.colors.colLayer1
    property bool placeholderShadow: false
    readonly property var player: MprisController.activePlayer
    readonly property bool hasPlayer: root.player !== null && root.player !== undefined

    implicitWidth: Appearance.sizes.mediaControlsWidth - Appearance.sizes.elevationMargin * 2
    implicitHeight: Appearance.sizes.mediaControlsHeight - Appearance.sizes.elevationMargin * 2

    readonly property string form: {
        const w = root.width;
        const h = root.height;
        if (h < 110)
            return w >= 140 && w >= h * 2 ? "row" : "circle";
        if (w >= 280 && w >= h * 1.5)
            return "full";
        if (w >= 170 && h >= 200)
            return "tall";
        return w >= 200 ? "row" : "circle";
    }
    // How much larger than the original's a card of the original's layout draws its content.
    readonly property real zoom: root.form === "full" ? Math.max(1, Math.min(2.5, root.width / root.implicitWidth, root.height / root.implicitHeight)) : root.form === "tall" ? Math.max(1, Math.min(2, root.width / 320)) : 1
    readonly property real circleSize: Math.min(root.width, root.height)

    Loader {
        id: playerLoader
        active: root.hasPlayer
        // PlayerControl draws its card inside a margin for its shadow: the card is this item.
        anchors.fill: parent
        anchors.margins: -Appearance.sizes.elevationMargin
        sourceComponent: PlayerControl {
            player: root.player
            radius: root.radius
            form: root.form
            zoom: root.zoom
        }
    }

    StyledRectangularShadow {
        target: placeholder
        visible: placeholder.visible && root.placeholderShadow
    }

    Rectangle {
        id: placeholder
        readonly property bool circle: root.form === "circle"
        readonly property bool low: root.height < 100
        visible: !root.hasPlayer
        anchors.centerIn: parent
        width: placeholder.circle ? root.circleSize : root.width
        height: placeholder.circle ? root.circleSize : root.height
        radius: placeholder.circle ? root.circleSize / 2 : root.radius
        color: root.placeholderColor

        MaterialSymbol {
            visible: placeholder.circle
            anchors.centerIn: parent
            text: "music_off"
            iconSize: Math.min(40, Math.round(root.circleSize * 0.5))
            color: Appearance.colors.colSubtext
        }

        RowLayout {
            visible: !placeholder.circle
            anchors.fill: parent
            anchors.margins: placeholder.low ? Math.min(16, Math.round(root.height * 0.16)) : Math.round(16 * root.zoom)
            spacing: placeholder.low ? 10 : Math.round(14 * root.zoom)

            MaterialSymbol {
                Layout.alignment: Qt.AlignVCenter
                text: "music_off"
                iconSize: placeholder.low ? Math.min(40, Math.round(root.height * 0.5)) : Math.round(40 * root.zoom)
                color: Appearance.colors.colSubtext
            }

            ColumnLayout {
                Layout.fillWidth: true
                Layout.alignment: Qt.AlignVCenter
                spacing: 2

                StyledText {
                    Layout.fillWidth: true
                    elide: Text.ElideRight
                    text: Translation.tr("No active player")
                    font.pixelSize: Math.round((placeholder.low && root.height < 64 ? Appearance.font.pixelSize.normal : Appearance.font.pixelSize.large) * root.zoom)
                    color: Appearance.colors.colOnLayer1
                }
                StyledText {
                    visible: !placeholder.low && root.width >= 280
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    maximumLineCount: 2
                    elide: Text.ElideRight
                    text: Translation.tr("Make sure your player has MPRIS support\nor try turning off duplicate player filtering")
                    font.pixelSize: Math.round(Appearance.font.pixelSize.small * root.zoom)
                    color: Appearance.colors.colSubtext
                }
            }
        }
    }
}
