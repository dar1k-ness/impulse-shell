import QtQuick
import QtQuick.Layouts
import Rustshell
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets

Rectangle {
    id: root

    property bool show: false
    default property alias contentData: contentColumn.data
    // Height of the open dialog: its content unless a dialog sets a fixed one.
    property real backgroundHeight: contentColumn.implicitHeight + dialogBackground.radius * 2
    property real backgroundWidth: 350
    property real backgroundAnimationMovementDistance: 60
    // The content is cut at the edge of the dialog while it opens and closes
    property alias clipsContent: dialogBackground.clip
    
    signal dismiss()
    Keys.onPressed: (event) => {
        if (event.key === Qt.Key_Escape) {
            root.dismiss();
            event.accepted = true;
        }
    }

    color: root.show ? Appearance.colors.colScrim : ColorUtils.transparentize(Appearance.colors.colScrim)
    Behavior on color {
        animation: Appearance.animation.elementMoveFast.colorAnimation.createObject(this)
    }
    visible: dialogBackground.implicitHeight > 0

    onShowChanged: {
        dialogBackgroundHeightAnimation.easing.bezierCurve = (show ? Appearance.animationCurves.emphasizedDecel : Appearance.animationCurves.emphasizedAccel)
    }

    radius: Appearance.rounding.screenRounding - Appearance.sizes.hyprlandGapsOut + 1

    MouseArea { // Clicking outside the dialog should dismiss
        anchors.fill: parent
        acceptedButtons: Qt.AllButtons
        hoverEnabled: true
        onPressed: root.dismiss()
    }

    Rectangle {
        id: dialogBackground
        anchors.horizontalCenter: parent.horizontalCenter
        radius: Appearance.rounding.large
        color: Appearance.m3colors.m3surfaceContainerHigh // Use opaque version of layer3
        
        // Whole pixels: an odd content height would otherwise centre the dialog on a half pixel.
        property real targetY: Math.round(root.height / 2 - root.backgroundHeight / 2)
        y: root.show ? targetY : (targetY - root.backgroundAnimationMovementDistance)
        implicitWidth: root.backgroundWidth
        // Bound rather than assigned on show: hidden until first shown, and an open dialog
        // follows content that arrives or changes after it opened.
        implicitHeight: root.show ? root.backgroundHeight : 0
        Behavior on implicitHeight {
            NumberAnimation {
                id: dialogBackgroundHeightAnimation
                duration: Appearance.animation.elementMoveFast.duration
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Appearance.animationCurves.emphasizedDecel
            }
        }
        Behavior on y {
            NumberAnimation {
                duration: dialogBackgroundHeightAnimation.duration
                easing.type: dialogBackgroundHeightAnimation.easing.type
                easing.bezierCurve: dialogBackgroundHeightAnimation.easing.bezierCurve
            }
        }

        MouseArea { // So clicking inside the dialog won't dismiss
            anchors.fill: parent
            acceptedButtons: Qt.AllButtons
            hoverEnabled: true
        }

        ColumnLayout {
            id: contentColumn
            anchors {
                fill: parent
                margins: dialogBackground.radius
            }
            spacing: 16
            opacity: root.show ? 1 : 0
            Behavior on opacity {
                animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
            }

        }
    }
}
