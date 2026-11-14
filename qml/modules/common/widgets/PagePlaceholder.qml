import QtQuick
import QtQuick.Layouts
import ri.modules.common
import ri.modules.common.widgets

Item {
    id: root

    property bool shown: true
    property alias icon: shapeWidget.text
    property alias title: widgetNameText.text
    property alias description: widgetDescriptionText.text
    property alias shape: shapeWidget.shape
    property alias descriptionHorizontalAlignment: widgetDescriptionText.horizontalAlignment
    property real horizontalPadding: 16

    opacity: shown ? 1 : 0
    visible: opacity > 0
    anchors {
        fill: parent
        topMargin: -30 * (1 - opacity)
        bottomMargin: 30 * (1 - opacity)
    }

    Behavior on opacity {
        animation: Appearance.animation.elementMoveEnter.numberAnimation.createObject(this)
    }

    ColumnLayout {
        anchors.centerIn: parent
        // A wrapping Text's implicitWidth is its unwrapped width: cap it so long descriptions wrap.
        width: Math.min(implicitWidth, root.width - 2 * root.horizontalPadding)
        spacing: 5

        MaterialShapeWrappedMaterialSymbol {
            id: shapeWidget
            Layout.alignment: Qt.AlignHCenter
            padding: 12
            iconSize: 56
            rotation: -30 * (1 - root.opacity)
        }
        StyledText {
            id: widgetNameText
            visible: title !== ""
            // A title wider than the page is read whole, on as many lines as it takes.
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            font {
                family: Appearance.font.family.title
                pixelSize: Appearance.font.pixelSize.larger
                variableAxes: Appearance.font.variableAxes.title
            }
            color: Appearance.m3colors.m3outline
            horizontalAlignment: Text.AlignHCenter
        }
        StyledText {
            id: widgetDescriptionText
            visible: description !== ""
            Layout.fillWidth: true
            font.pixelSize: Appearance.font.pixelSize.small
            color: Appearance.m3colors.m3outline
            horizontalAlignment: Text.AlignLeft
            wrapMode: Text.Wrap
        }
    }
}
