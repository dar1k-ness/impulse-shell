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
 * (modules/rusty/editMode/StyleReviewBar.qml): like, dislike (which picks another by the same
 * request), confirm, and back out (also Esc), which puts the look from before the pick back.
 * The review ends with the edit mode, however that is left. Nothing here runs until a pick ends.
 */
Singleton {
    id: root

    property bool open: false
    // The style on the screen has been liked.
    property bool liked: false
    // Whether the last answer of the picker belongs to this review (and not to a run before it).
    property bool answered: false
    // The review is being left by backing out, not by confirming.
    property bool dropped: false
    readonly property bool busy: Wallpick.busy
    readonly property bool failed: root.answered && !Wallpick.busy && Wallpick.failed
    readonly property string status: root.answered ? Wallpick.status : ""

    // Called by the family that has the edit mode, so the IPC target exists from its start on.
    function load() {
    }

    function begin() {
        // The edit mode is loaded for a layout with containers only (ImpulseShellFamily.qml).
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

    // The picked style stays, with the widgets as they were arranged.
    function confirm() {
        root.dropped = false;
        controller.finish();
    }

    // The user backs out: the look from before the pick comes back.
    function cancel() {
        if (!root.open)
            return;
        root.dropped = true;
        controller.finish();
    }

    // Esc is about to leave the edit mode (EditOverlay.qml), which backs out as well; taken
    // back when the edit mode did not take the key.
    function leaving(byEsc) {
        root.dropped = root.open && byEsc;
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
                // Backing out puts the earlier look back; otherwise the style stays, unless a
                // pick is still running: its style is reviewed when it arrives.
                if (root.open && root.dropped)
                    Wallpick.revert();
                else if (root.open && !Wallpick.busy)
                    Wallpick.keep();
                root.open = false;
                root.liked = false;
                root.answered = false;
                root.dropped = false;
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
