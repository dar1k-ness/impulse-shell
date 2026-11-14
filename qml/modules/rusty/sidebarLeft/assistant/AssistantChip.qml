import ri.modules.common
import ri.modules.common.widgets
import QtQuick
import QtQuick.Layouts

RippleButton {
    id: chip
    property string chipIcon
    property string chipLabel
    property string tooltip
    implicitHeight: 30
    implicitWidth: chipRow.implicitWidth + 10 * 2
    buttonRadius: Appearance.rounding.full
    colBackground: Appearance.colors.colLayer2
    colBackgroundHover: Appearance.colors.colLayer2Hover
    colBackgroundToggled: Appearance.colors.colSecondaryContainer
    colBackgroundToggledHover: Appearance.colors.colSecondaryContainerHover
    contentItem: Item {
        implicitWidth: chipRow.implicitWidth
        implicitHeight: chipRow.implicitHeight
        RowLayout {
            id: chipRow
            anchors.centerIn: parent
            width: Math.min(implicitWidth, chip.width - 10 * 2)
            spacing: 4
            MaterialSymbol {
                text: chip.chipIcon
                iconSize: Appearance.font.pixelSize.larger
                color: chip.toggled ? Appearance.m3colors.m3onSecondaryContainer : Appearance.colors.colOnLayer2
            }
            StyledText {
                visible: chip.chipLabel.length > 0
                Layout.fillWidth: true
                elide: Text.ElideRight
                font.pixelSize: Appearance.font.pixelSize.smaller
                color: chip.toggled ? Appearance.m3colors.m3onSecondaryContainer : Appearance.colors.colOnLayer2
                text: chip.chipLabel
            }
            MaterialSymbol {
                visible: chip.chipLabel.length > 0
                text: "expand_more"
                iconSize: Appearance.font.pixelSize.normal
                color: Appearance.colors.colSubtext
            }
        }
    }
    StyledToolTip {
        text: chip.tooltip
        extraVisibleCondition: chip.tooltip.length > 0
    }
}
