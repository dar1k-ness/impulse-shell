import QtQuick
import ri
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.rusty.bar as Bar

/**
 * Desktop widget "resources": RAM, swap and CPU usage gauges on a card filling the box the
 * desktop container gives it; without a size it takes the gauges' own. In a box of another
 * shape the gauges take the arrangement that draws them largest there (one above another in
 * a high box, their labels beside the rings in a low one, the rings alone in a box too small
 * for labels), drawn larger in a larger box and
 * scaled down in one too small for them. Hidden on the lock screen, like the original's
 * background widgets.
 */
Item {
    id: root
    // The card's padding at its own size; a small box keeps less.
    property real naturalPadding: 20
    readonly property real padding: root.sized && root.width > 0 && root.height > 0 ? Math.min(root.naturalPadding, Math.round(Math.min(root.width, root.height) * 0.2)) : root.naturalPadding
    // From the container (WidgetHost.placement): the box has a size of the layout's. Without
    // one the gauges keep the arrangement and the size their own size is measured in.
    property bool sized: false
    readonly property var arrangement: root.sized ? card.fitTo(Math.max(1, root.width - root.padding * 2), Math.max(1, root.height - root.padding * 2)) : ({
            "stacked": false,
            "labelsBeside": false,
            "ringsOnly": false,
            "zoom": 1
        })
    implicitWidth: card.implicitWidth + root.naturalPadding * 2
    implicitHeight: card.implicitHeight + root.naturalPadding * 2
    visible: opacity > 0
    opacity: GlobalStates.screenLocked ? 0 : 1
    Behavior on opacity {
        animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
    }

    StyledRectangularShadow {
        target: background
    }

    Rectangle {
        id: background
        anchors.fill: parent
        radius: Appearance.rounding.large
        color: Appearance.colors.colLayer0

        Bar.ResourcesCard {
            id: card
            anchors.centerIn: parent
            colOnCard: Appearance.colors.colOnLayer0
            stacked: root.arrangement.stacked
            labelsBeside: root.arrangement.labelsBeside
            ringsOnly: root.arrangement.ringsOnly
            zoom: root.arrangement.zoom
            // What the arrangement leaves over, and all of it in a box smaller than the gauges.
            scale: Math.min((root.width - root.padding * 2) / Math.max(1, card.implicitWidth), (root.height - root.padding * 2) / Math.max(1, card.implicitHeight))
        }
    }
}
