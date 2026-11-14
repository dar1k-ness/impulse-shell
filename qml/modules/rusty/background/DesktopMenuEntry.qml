import QtQuick
import QtQuick.Layouts
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.common.functions

// One entry of the desktop menu, drawn as the tray menu's entries (SysTrayMenuEntry).
RippleButton {
    id: root

    property string symbol
    property string label

    signal triggered

    colBackground: ColorUtils.transparentize(Appearance.colors.colLayer0)
    horizontalPadding: 12
    implicitWidth: content.implicitWidth + horizontalPadding * 2
    implicitHeight: 36
    Layout.fillWidth: true

    releaseAction: () => root.triggered()

    contentItem: RowLayout {
        id: content
        anchors {
            verticalCenter: parent.verticalCenter
            left: parent.left
            right: parent.right
            leftMargin: root.horizontalPadding
            rightMargin: root.horizontalPadding
        }
        spacing: 8

        MaterialSymbol {
            text: root.symbol
            iconSize: 20
        }

        StyledText {
            text: root.label
            font.pixelSize: Appearance.font.pixelSize.smallie
            Layout.fillWidth: true
        }
    }
}
