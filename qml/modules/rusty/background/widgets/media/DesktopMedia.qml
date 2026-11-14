import QtQuick
import ri
import ri.modules.common
import ri.modules.rusty.mediaControls

/**
 * Desktop widget "media": the original's media controls for the active player as a card filling the
 * box the desktop container gives it; without a size it takes the media controls' own.
 * Hidden on the lock screen, like the original's background widgets.
 */
Item {
    id: root
    implicitWidth: media.implicitWidth
    implicitHeight: media.implicitHeight
    visible: opacity > 0
    opacity: GlobalStates.screenLocked ? 0 : 1
    Behavior on opacity {
        animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
    }

    MediaCard {
        id: media
        anchors.fill: parent
        radius: Appearance.rounding.screenRounding - Appearance.sizes.hyprlandGapsOut + 1
        placeholderColor: Appearance.colors.colLayer0
        placeholderShadow: true
    }
}
