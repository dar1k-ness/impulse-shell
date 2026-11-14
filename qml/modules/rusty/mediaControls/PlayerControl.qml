pragma ComponentBehavior: Bound
import ri.modules.common
import ri.modules.common.models
import ri.modules.common.widgets
import ri.services
import ri.modules.common.functions
import Qt5Compat.GraphicalEffects
import Impulse
import QtQuick
import QtQuick.Effects
import QtQuick.Layouts
import Rustshell
import Rustshell.Io
import Rustshell.Services.Mpris

Item { // Player instance
    id: root
    required property MprisPlayer player
    property var artUrl: player?.trackArtUrl
    property string artDownloadLocation: Directories.coverArt
    property string artFileName: Qt.md5(artUrl)
    property string artFilePath: `${artDownloadLocation}/${artFileName}`
    property color artDominantColor: ColorUtils.mix((colorQuantizer?.colors[0] ?? Appearance.colors.colPrimary), Appearance.colors.colPrimaryContainer, 0.8) || Appearance.m3colors.m3secondaryContainer
    property bool downloaded: false
    property list<real> visualizerPoints: []
    property real maxVisualizerValue: 1000 // Max value in the data points
    property int visualizerSmoothing: 2 // Number of points to average for smoothing
    property real radius
    // The layout the card takes and how much larger than the original's it draws its content
    // (MediaCard picks both by the card's size); the original's own are "full" at 1.
    property string form: "full"
    property real zoom: 1
    readonly property real cardWidth: Math.max(0, root.width - Appearance.sizes.elevationMargin * 2)
    readonly property real cardHeight: Math.max(0, root.height - Appearance.sizes.elevationMargin * 2)
    readonly property bool circle: root.form === "circle"
    readonly property real circleSize: Math.min(root.cardWidth, root.cardHeight)

    property string displayedArtFilePath: root.downloaded ? Qt.resolvedUrl(artFilePath) : ""

    component TrackChangeButton: RippleButton {
        implicitWidth: Math.round(24 * root.zoom)
        implicitHeight: Math.round(24 * root.zoom)

        property var iconName
        colBackground: ColorUtils.transparentize(blendedColors.colSecondaryContainer, 1)
        colBackgroundHover: blendedColors.colSecondaryContainerHover
        colRipple: blendedColors.colSecondaryContainerActive

        contentItem: MaterialSymbol {
            iconSize: Math.round(Appearance.font.pixelSize.huge * root.zoom)
            fill: 1
            horizontalAlignment: Text.AlignHCenter
            color: blendedColors.colOnSecondaryContainer
            text: iconName

            Behavior on color {
                animation: Appearance.animation.elementMoveFast.colorAnimation.createObject(this)
            }
        }
    }

    component PlayPauseButton: RippleButton {
        id: playPause
        property real size: 44
        implicitWidth: size
        implicitHeight: size
        downAction: () => root.player.togglePlaying();

        buttonRadius: root.player?.isPlaying ? Math.min(Appearance?.rounding.normal, size / 2) : size / 2
        colBackground: root.player?.isPlaying ? blendedColors.colPrimary : blendedColors.colSecondaryContainer
        colBackgroundHover: root.player?.isPlaying ? blendedColors.colPrimaryHover : blendedColors.colSecondaryContainerHover
        colRipple: root.player?.isPlaying ? blendedColors.colPrimaryActive : blendedColors.colSecondaryContainerActive

        contentItem: MaterialSymbol {
            iconSize: Math.round(Math.min(Appearance.font.pixelSize.huge * root.zoom, playPause.size * 0.6))
            fill: 1
            horizontalAlignment: Text.AlignHCenter
            color: root.player?.isPlaying ? blendedColors.colOnPrimary : blendedColors.colOnSecondaryContainer
            text: root.player?.isPlaying ? "pause" : "play_arrow"

            Behavior on color {
                animation: Appearance.animation.elementMoveFast.colorAnimation.createObject(this)
            }
        }
    }

    // The cover in a rounded box of its own.
    component ArtBox: Rectangle {
        id: artBox
        radius: Appearance.rounding.verysmall
        color: ColorUtils.transparentize(blendedColors.colLayer1, 0.5)

        layer.enabled: true
        layer.effect: OpacityMask {
            maskSource: Rectangle {
                width: artBox.width
                height: artBox.height
                radius: artBox.radius
            }
        }

        StyledImage {
            anchors.fill: parent
            source: root.displayedArtFilePath
            fillMode: Image.PreserveAspectCrop
            cache: false
            antialiasing: true
        }
    }

    // The seek slider of a player that can seek, the progress bar of one that cannot.
    component SeekBar: Item {
        implicitHeight: Math.max(seekSlider.implicitHeight, seekProgress.implicitHeight)

        Loader {
            id: seekSlider
            anchors.fill: parent
            active: root.player?.canSeek ?? false
            sourceComponent: StyledSlider {
                configuration: StyledSlider.Configuration.Wavy
                highlightColor: blendedColors.colPrimary
                trackColor: blendedColors.colSecondaryContainer
                handleColor: blendedColors.colPrimary
                value: root.player?.position / root.player?.length
                onMoved: {
                    root.player.position = value * root.player.length;
                }
            }
        }

        Loader {
            id: seekProgress
            anchors {
                verticalCenter: parent.verticalCenter
                left: parent.left
                right: parent.right
            }
            active: !(root.player?.canSeek ?? false)
            sourceComponent: StyledProgressBar {
                wavy: root.player?.isPlaying
                highlightColor: blendedColors.colPrimary
                trackColor: blendedColors.colSecondaryContainer
                value: root.player?.position / root.player?.length
            }
        }
    }

    component TrackTitle: StyledText {
        font.pixelSize: Math.round(Appearance.font.pixelSize.large * root.zoom)
        color: blendedColors.colOnLayer0
        elide: Text.ElideRight
        text: StringUtils.cleanMusicTitle(root.player?.trackTitle) || "Untitled"
        animateChange: true
        animationDistanceX: 6
        animationDistanceY: 0
    }

    component TrackArtist: StyledText {
        font.pixelSize: Math.round(Appearance.font.pixelSize.smaller * root.zoom)
        color: blendedColors.colSubtext
        elide: Text.ElideRight
        text: root.player?.trackArtist
        animateChange: true
        animationDistanceX: 6
        animationDistanceY: 0
    }

    UpdateTimer { // Force update for revision
        running: root.player?.playbackState == MprisPlaybackState.Playing
        onTriggered: {
            root.player.positionChanged()
        }
    }

    onArtFilePathChanged: {
        if (root.artUrl.length == 0) {
            root.artDominantColor = Appearance.m3colors.m3secondaryContainer
            return;
        }

        // Binding does not work in Process
        coverArtDownloader.url = root.artUrl
        coverArtDownloader.path = root.artFilePath
        // Download
        root.downloaded = false
        coverArtDownloader.running = true
    }

    Download { // Cover art downloader
        id: coverArtDownloader
        url: root.artUrl
        path: root.artFilePath
        ipv4: true
        onExited: (exitCode, exitStatus) => {
            root.downloaded = true
        }
    }

    ColorQuantizer {
        id: colorQuantizer
        source: root.displayedArtFilePath
        depth: 0 // 2^0 = 1 color
        rescaleSize: 1 // Rescale to 1x1 pixel for faster processing
    }

    property QtObject blendedColors: AdaptedMaterialScheme {
        color: artDominantColor
    }

    StyledRectangularShadow {
        target: background
    }
    Rectangle { // Background
        id: background
        anchors.centerIn: parent
        width: root.circle ? root.circleSize : root.cardWidth
        height: root.circle ? root.circleSize : root.cardHeight
        color: ColorUtils.applyAlpha(blendedColors.colLayer0, 1)
        radius: root.circle ? root.circleSize / 2 : root.radius

        layer.enabled: true
        layer.effect: OpacityMask {
            maskSource: Rectangle {
                width: background.width
                height: background.height
                radius: background.radius
            }
        }

        StyledImage {
            id: blurredArt
            anchors.fill: parent
            source: root.displayedArtFilePath
            fillMode: Image.PreserveAspectCrop
            cache: false
            antialiasing: true
            asynchronous: true

            layer.enabled: true
            layer.effect: StyledBlurEffect {
                source: blurredArt
            }

            Rectangle {
                anchors.fill: parent
                color: ColorUtils.transparentize(blendedColors.colLayer0, 0.3)
                radius: root.radius
            }
        }

        WaveVisualizer {
            id: visualizerCanvas
            visible: !root.circle
            anchors.fill: parent
            live: root.player?.isPlaying
            points: root.visualizerPoints
            maxVisualizerValue: root.maxVisualizerValue
            smoothing: root.visualizerSmoothing
            color: blendedColors.colPrimary
        }

        RowLayout {
            id: fullLayout
            // The title and the artist stop short of the play button where the card is too
            // low for them to pass above it.
            readonly property real textInset: root.cardHeight < Appearance.sizes.mediaControlsHeight - Appearance.sizes.elevationMargin * 2 ? playPauseButton.width + 6 : 0
            visible: root.form === "full"
            anchors.fill: parent
            anchors.margins: Math.round(13 * root.zoom)
            spacing: Math.round(15 * root.zoom)

            ArtBox { // Art background
                // As high as the card lets it, less where the text beside it would be left no room.
                readonly property real side: Math.max(0, Math.min(fullLayout.height, fullLayout.width - fullLayout.spacing - Math.round(135 * root.zoom)))
                Layout.alignment: Qt.AlignVCenter
                Layout.preferredWidth: side
                Layout.preferredHeight: side
            }

            ColumnLayout { // Info & controls
                Layout.fillHeight: true
                spacing: 2

                TrackTitle {
                    id: trackTitle
                    Layout.fillWidth: true
                    Layout.rightMargin: fullLayout.textInset
                }
                TrackArtist {
                    id: trackArtist
                    Layout.fillWidth: true
                    Layout.rightMargin: fullLayout.textInset
                }
                Item { Layout.fillHeight: true }
                Item {
                    Layout.fillWidth: true
                    implicitHeight: trackTime.implicitHeight + sliderRow.implicitHeight

                    StyledText {
                        id: trackTime
                        anchors.bottom: sliderRow.top
                        anchors.bottomMargin: 5
                        anchors.left: parent.left
                        anchors.right: playPauseButton.left
                        font.pixelSize: Math.round(Appearance.font.pixelSize.small * root.zoom)
                        color: blendedColors.colSubtext
                        elide: Text.ElideRight
                        text: `${StringUtils.friendlyTimeForSeconds(root.player?.position)} / ${StringUtils.friendlyTimeForSeconds(root.player?.length)}`
                    }
                    RowLayout {
                        id: sliderRow
                        anchors {
                            bottom: parent.bottom
                            left: parent.left
                            right: parent.right
                        }
                        TrackChangeButton {
                            iconName: "skip_previous"
                            downAction: () => root.player?.previous()
                        }
                        SeekBar {
                            Layout.fillWidth: true
                        }
                        TrackChangeButton {
                            iconName: "skip_next"
                            downAction: () => root.player?.next()
                        }
                    }

                    PlayPauseButton {
                        id: playPauseButton
                        anchors.right: parent.right
                        anchors.bottom: sliderRow.top
                        anchors.bottomMargin: 5
                        size: Math.round(44 * root.zoom)
                    }
                }
            }
        }

        // A narrow high card: the cover above the title, the play button and the seek bar.
        Loader {
            id: tallLoader
            active: root.form === "tall"
            anchors.fill: parent
            anchors.margins: 13
            sourceComponent: ColumnLayout {
                id: tallLayout
                readonly property real controlsHeight: Math.round(44 * root.zoom) + Math.round(24 * root.zoom) + tallLayout.spacing * 2
                spacing: 8

                ArtBox {
                    readonly property real side: Math.max(0, Math.min(tallLoader.width, tallLoader.height - tallLayout.controlsHeight))
                    Layout.alignment: Qt.AlignHCenter
                    Layout.preferredWidth: side
                    Layout.preferredHeight: side
                }
                Item { Layout.fillHeight: true }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignVCenter
                        spacing: 2

                        TrackTitle {
                            Layout.fillWidth: true
                        }
                        TrackArtist {
                            Layout.fillWidth: true
                        }
                    }
                    PlayPauseButton {
                        size: Math.round(44 * root.zoom)
                    }
                }
                RowLayout {
                    Layout.fillWidth: true

                    TrackChangeButton {
                        iconName: "skip_previous"
                        downAction: () => root.player?.previous()
                    }
                    SeekBar {
                        Layout.fillWidth: true
                    }
                    TrackChangeButton {
                        iconName: "skip_next"
                        downAction: () => root.player?.next()
                    }
                }
            }
        }

        // A low card: the cover, the title and the artist beside it, and the buttons there is
        // room for; without a play button a click anywhere on the card plays and pauses.
        Loader {
            id: rowLoader
            readonly property real pad: Math.min(13, Math.round(root.cardHeight * 0.16))
            active: root.form === "row"
            anchors.fill: parent
            sourceComponent: Item {
                id: rowForm
                readonly property real inner: Math.max(0, rowForm.height - rowLoader.pad * 2)
                readonly property real artSide: Math.max(0, Math.min(rowForm.inner, rowForm.width - rowLoader.pad * 2 - rowLayout.spacing - 96))
                readonly property real buttonSide: Math.min(44, rowForm.inner)
                // What is left for the text once the cover has its place.
                readonly property real textRoom: rowForm.width - rowLoader.pad * 2 - rowForm.artSide - rowLayout.spacing
                readonly property bool hasPlayButton: rowForm.textRoom - rowForm.buttonSide - rowLayout.spacing >= 90
                readonly property bool hasSkipButtons: rowForm.textRoom - rowForm.buttonSide - 48 - rowLayout.spacing * 3 >= 180

                MouseArea {
                    anchors.fill: parent
                    enabled: !rowForm.hasPlayButton
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.player.togglePlaying()
                }

                RowLayout {
                    id: rowLayout
                    anchors.fill: parent
                    anchors.margins: rowLoader.pad
                    spacing: 10

                    ArtBox {
                        Layout.alignment: Qt.AlignVCenter
                        Layout.preferredWidth: rowForm.artSide
                        Layout.preferredHeight: rowForm.artSide
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignVCenter
                        spacing: 0

                        TrackTitle {
                            Layout.fillWidth: true
                            font.pixelSize: rowForm.inner < 44 ? Appearance.font.pixelSize.small : Appearance.font.pixelSize.large
                        }
                        TrackArtist {
                            visible: rowForm.inner >= 36 && text.length > 0
                            Layout.fillWidth: true
                        }
                    }
                    TrackChangeButton {
                        visible: rowForm.hasSkipButtons
                        Layout.alignment: Qt.AlignVCenter
                        iconName: "skip_previous"
                        downAction: () => root.player?.previous()
                    }
                    PlayPauseButton {
                        visible: rowForm.hasPlayButton
                        Layout.alignment: Qt.AlignVCenter
                        size: rowForm.buttonSide
                    }
                    TrackChangeButton {
                        visible: rowForm.hasSkipButtons
                        Layout.alignment: Qt.AlignVCenter
                        iconName: "skip_next"
                        downAction: () => root.player?.next()
                    }
                }
            }
        }

        // No room for text: the cover alone, round, with the play button in its middle.
        Loader {
            active: root.circle
            anchors.fill: parent
            sourceComponent: Item {
                StyledImage {
                    anchors.fill: parent
                    source: root.displayedArtFilePath
                    fillMode: Image.PreserveAspectCrop
                    cache: false
                    antialiasing: true
                }
                PlayPauseButton {
                    anchors.centerIn: parent
                    size: Math.round(Math.min(44, root.circleSize * 0.55))
                }
            }
        }
    }
}