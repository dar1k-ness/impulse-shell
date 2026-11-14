pragma Singleton
import ri.modules.common
import QtQuick
import Rustshell
import Rustshell.Wayland

/**
 * A nice wrapper for date and time strings.
 */
Singleton {
    id: root

    property alias inhibit: idleInhibitor.enabled
    inhibit: false

    Connections {
        target: Persistent
        function onReadyChanged() {
            if (!Persistent.isNewHyprlandInstance) {
                root.inhibit = Persistent.states.idle.inhibit;
            } else {
                Persistent.states.idle.inhibit = root.inhibit;
            }
        }
    }

    function toggleInhibit(active = null) {
        if (active !== null) {
            root.inhibit = active;
        } else {
            root.inhibit = !root.inhibit;
        }
        Persistent.states.idle.inhibit = root.inhibit;
    }

    // The compositor closes the window with the monitor it is on, and a closed window stays
    // closed: "Keep awake" would stay on and inhibit nothing (docs/original-differences.md).
    function reopenWindow() {
        if (!inhibitorWindow.visible && Rustshell.screens.length > 0)
            inhibitorWindow.visible = true;
    }
    readonly property int screenCount: Rustshell.screens.length
    onScreenCountChanged: reopenWindow()

    IdleInhibitor {
        id: idleInhibitor
        window: PanelWindow {
            id: inhibitorWindow
            onClosed: Qt.callLater(root.reopenWindow)
            // Inhibitor requires a "visible" surface
            // Actually not lol
            implicitWidth: 0
            implicitHeight: 0
            color: "transparent"
            // Just in case...
            anchors {
                right: true
                bottom: true
            }
            // Make it not interactable
            mask: Region {
                item: null
            }
        }
    }
}
