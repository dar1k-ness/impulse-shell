import QtQuick
import QtQuick.Layouts
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets
import ri.services

/**
 * What "Pick for me" asks before it picks, as a dialog over the settings window
 * (`Wallpick.asking`): a style that goes by the desktop as it is now, or by the user's own
 * words (any language). The picker remembers the answer for "dislike", which picks again by
 * the same.
 */
WindowDialog {
    id: root
    show: Wallpick.asking
    backgroundWidth: Math.min(520, width - 48)
    clipsContent: true
    // Over a window, not over a screen's panel: square corners
    radius: 0
    onDismiss: Wallpick.asking = false
    onShowChanged: {
        if (show)
            input.forceActiveFocus();
        else
            input.focus = false;
    }

    // The picture of the desktop: a video wallpaper's still frame
    readonly property string wallpaper: {
        const bg = Config.options.background;
        return /\.(mp4|webm|mkv|avi|mov)$/.test(bg.wallpaperPath) ? bg.thumbnailPath : bg.wallpaperPath;
    }
    readonly property var examples: [
        Translation.tr("night city in the rain"),
        Translation.tr("mountains in the fog"),
        Translation.tr("calm pastel sky")
    ]

    function byDesktop() {
        if (Wallpick.busy)
            return;
        Wallpick.pickByDesktop();
        Wallpick.asking = false;
    }
    function byWords() {
        const words = input.text.trim();
        if (words.length === 0 || Wallpick.busy)
            return;
        Config.options.background.wallpick.request = words;
        Wallpick.pickByWords(words);
        Wallpick.asking = false;
    }

    WindowDialogTitle {
        Layout.fillWidth: true
        text: Translation.tr("What should the new style go by?")
    }

    // The desktop as it is now, shown by its wallpaper
    RippleButton {
        id: desktop
        Layout.fillWidth: true
        implicitHeight: 76
        enabled: !Wallpick.busy
        buttonRadius: Appearance.rounding.normal
        colBackground: Appearance.colors.colSurfaceContainerHighest
        colBackgroundHover: Appearance.colors.colSurfaceContainerHighestHover
        onClicked: root.byDesktop()
        contentItem: RowLayout {
            spacing: 14
            Rectangle {
                implicitWidth: 92
                implicitHeight: 52
                radius: Appearance.rounding.small
                color: Appearance.colors.colLayer1
                clip: true
                MaterialSymbol {
                    anchors.centerIn: parent
                    visible: thumb.status !== Image.Ready
                    text: "desktop_windows"
                    iconSize: Appearance.font.pixelSize.hugeass
                    color: Appearance.colors.colSubtext
                }
                Image {
                    id: thumb
                    anchors.fill: parent
                    visible: status === Image.Ready
                    // Kept loaded: decoding it as the dialog opens would hold the opening up
                    source: root.wallpaper
                    sourceSize.width: 184
                    fillMode: Image.PreserveAspectCrop
                    asynchronous: true
                }
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                StyledText {
                    Layout.fillWidth: true
                    text: Translation.tr("My desktop as it is now")
                    color: Appearance.colors.colOnLayer2
                    elide: Text.ElideRight
                }
                StyledText {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    text: Translation.tr("Looks for wallpapers like the one on the desktop now")
                    color: Appearance.colors.colSubtext
                    font.pixelSize: Appearance.font.pixelSize.smaller
                }
            }
            MaterialSymbol {
                text: "chevron_right"
                iconSize: Appearance.font.pixelSize.larger
                color: Appearance.colors.colSubtext
            }
        }
    }

    ColumnLayout {
        Layout.fillWidth: true
        spacing: 8
        StyledText {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            text: Translation.tr("Or describe it in your words")
            color: Appearance.colors.colOnLayer2
            font.pixelSize: Appearance.font.pixelSize.small
        }
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 44
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
                Keys.onEscapePressed: root.dismiss()
                onAccepted: root.byWords()
            }
            StyledText {
                anchors.fill: input
                verticalAlignment: Text.AlignVCenter
                visible: input.text.length === 0
                text: root.examples[0]
                color: Appearance.colors.colSubtext
                font.pixelSize: Appearance.font.pixelSize.small
                elide: Text.ElideRight
            }
        }
        // Examples: a press puts one into the field
        Flow {
            Layout.fillWidth: true
            spacing: 6
            Repeater {
                model: root.examples
                RippleButton {
                    id: example
                    required property string modelData
                    implicitHeight: 28
                    implicitWidth: exampleText.implicitWidth + 24
                    buttonRadius: Appearance.rounding.full
                    colBackground: Appearance.colors.colSurfaceContainerHighest
                    colBackgroundHover: Appearance.colors.colSurfaceContainerHighestHover
                    onClicked: {
                        input.text = example.modelData;
                        input.forceActiveFocus();
                    }
                    contentItem: StyledText {
                        id: exampleText
                        horizontalAlignment: Text.AlignHCenter
                        verticalAlignment: Text.AlignVCenter
                        text: example.modelData
                        color: Appearance.colors.colOnLayer2
                        font.pixelSize: Appearance.font.pixelSize.smaller
                    }
                }
            }
        }
    }

    WindowDialogButtonRow {
        Layout.fillWidth: true
        Item {
            Layout.fillWidth: true
        }
        DialogButton {
            buttonText: Translation.tr("Cancel")
            onClicked: root.dismiss()
        }
        DialogButton {
            buttonText: Translation.tr("By these words")
            enabled: !Wallpick.busy && input.text.trim().length > 0
            onClicked: root.byWords()
        }
    }
}
