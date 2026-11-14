import QtQuick
import ri.modules.common
import ri.modules.common.models.quickToggles
import ri.modules.common.widgets
import ri.modules.rusty.bar

// A quick toggle on a panel: the original's bar util-button form (a circle with the toggle's icon),
// filled with the primary color while on, as its sidebar tile is.
CircleUtilButton {
    id: root
    property QuickToggleModel toggleModel
    // Keeps its circle instead of filling the panel's thickness.
    readonly property bool fillThickness: false

    toggled: root.toggleModel?.toggled ?? false
    enabled: root.toggleModel?.available ?? true
    colBackgroundToggled: Appearance.colors.colPrimary
    colBackgroundToggledHover: Appearance.colors.colPrimaryHover
    onClicked: {
        if (root.toggleModel && root.toggleModel.mainAction)
            root.toggleModel.mainAction();
    }

    MaterialSymbol {
        horizontalAlignment: Qt.AlignHCenter
        fill: root.toggled ? 1 : 0
        text: root.toggleModel?.icon ?? "close"
        iconSize: Appearance.font.pixelSize.large
        color: root.toggled ? Appearance.colors.colOnPrimary : Appearance.colors.colOnLayer2
    }
}
