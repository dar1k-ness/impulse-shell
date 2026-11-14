pragma Singleton

import QtQuick
import Impulse
import Rustshell
import Rustshell.Hyprland
import Rustshell.Io
import ri.modules.common

/**
 * The review of a style the wallpaper picker has just put on the screen (services/Wallpick.qml):
 * the shell goes to an empty workspace, where nothing covers the new wallpaper and the desktop
 * widgets, and opens the layout edit mode, whose overlay shows the review's bar
 * (modules/rusty/editMode/StyleReviewBar.qml): like, dislike (which picks another) and confirm.
 * The review ends with the edit mode, however that is left. Nothing here runs until a pick ends.
 */
Singleton {
    id: root

    property bool open: false
    // The style on the screen has been liked.
    property bool liked: false
    // Whether the last answer of the picker belongs to this review (and not to a run before it).
    property bool answered: false
    readonly property bool busy: Wallpick.busy
    readonly property bool failed: root.answered && !Wallpick.busy && Wallpick.failed
    readonly property string status: root.answered ? Wallpick.status : ""

    // Called by the family that has the edit mode, so the IPC target exists from its start on.
    function load() {
    }

    function begin() {
        // The edit mode is loaded for a layout with containers only (RustyImpulseFamily.qml).
        if (LayoutState.containerKinds.length === 0)
            return;
        root.liked = false;
        root.answered = false;
        if (!root.open) {
            root.open = true;
            Hyprland.dispatch(`hl.dsp.focus({ workspace = "empty" })`);
        }
        controller.start();
    }

    function like() {
        if (Wallpick.busy || root.liked)
            return;
        root.answered = true;
        Wallpick.like();
    }

    // Wallpick.finished() opens the review again once the next style is on the screen.
    function dislike() {
        if (Wallpick.busy)
            return;
        root.answered = true;
        root.liked = false;
        Wallpick.dislike();
    }

    function confirm() {
        controller.finish();
    }

    Connections {
        target: Wallpick
        function onEnded(args, ok) {
            if (root.open && args[0] === "like")
                root.liked = ok;
        }
    }

    EditController {
        id: controller
        onActiveChanged: {
            if (!active) {
                root.open = false;
                root.liked = false;
                root.answered = false;
            }
        }
    }

    IpcHandler {
        target: "styleReview"

        function open(): void {
            root.begin();
        }
    }
}
