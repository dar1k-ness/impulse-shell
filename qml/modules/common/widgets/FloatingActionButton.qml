import QtQuick
import QtQuick.Layouts
import ri.modules.common
import ri.modules.common.widgets

/**
 * Material 3 FAB.
 */
RippleButton {
    id: root
    property string iconText: "add"
    property bool expanded: false
    property real baseSize: 56
    property real elementSpacing: 5
    // A label the button switches to (a confirmation): reservedWidth fits it and buttonText
    property string reservedText: ""
    // The expanded width for the wider of buttonText and reservedText: a container sized to it
    // keeps its width while the label switches
    readonly property real reservedWidth: expanded ? Math.max(icon.width + Math.max(labelMetrics.advanceWidth, reservedMetrics.advanceWidth) + elementSpacing + contentRowLayout.horizontalMargins + 10 * 2, baseSize) : baseSize
    implicitWidth: expanded ? (Math.max(contentRowLayout.implicitWidth + 10 * 2, baseSize)) : baseSize
    implicitHeight: baseSize
    buttonRadius: baseSize / 14 * 4
    colBackground: Appearance.colors.colPrimaryContainer
    colBackgroundHover: Appearance.colors.colPrimaryContainerHover
    colRipple: Appearance.colors.colPrimaryContainerActive
    property color colOnBackground: Appearance.colors.colOnPrimaryContainer

    // The label's font (as buttonText's below), for measuring both labels
    StyledText {
        id: labelFont
        visible: false
        font.pixelSize: 14
        font.weight: 450
    }
    TextMetrics {
        id: labelMetrics
        font: labelFont.font
        text: root.buttonText
    }
    TextMetrics {
        id: reservedMetrics
        font: labelFont.font
        text: root.reservedText
    }

    contentItem: Row {
        id: contentRowLayout
        property real horizontalMargins: (root.baseSize - icon.width) / 2
        anchors {
            verticalCenter: parent?.verticalCenter
            left: parent?.left
            leftMargin: contentRowLayout.horizontalMargins
        }
        spacing: 0

        MaterialSymbol {
            id: icon
            anchors.verticalCenter: parent.verticalCenter
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
            iconSize: 26
            color: root.colOnBackground
            text: root.iconText
        }
        Loader {
            anchors.verticalCenter: parent.verticalCenter
            visible: root.buttonText?.length > 0
            active: true
            sourceComponent: Revealer {
                visible: root.expanded || implicitWidth > 0
                reveal: root.expanded
                implicitWidth: reveal ? (buttonText.implicitWidth + root.elementSpacing + contentRowLayout.horizontalMargins) : 0
                StyledText {
                    id: buttonText
                    anchors {
                        left: parent.left
                        leftMargin: root.elementSpacing
                        verticalCenter: parent.verticalCenter
                    }
                    text: root.buttonText
                    color: Appearance.colors.colOnPrimaryContainer
                    font.pixelSize: 14
                    font.weight: 450
                }
            }
        }
    }
}
