pragma Singleton
pragma ComponentBehavior: Bound

import ri.modules.common
import QtQuick
import Rustshell
import Rustshell.Io
import Impulse

/**
 * The one way the original switches the wallpaper and its colors: one run at a time. A request made while a run is going
 * waits for it, and a newer request replaces an older waiting one, so the last request is the
 * one whose wallpaper and colors stay. settled() comes once the last run has exited and the
 * colors file it wrote has been read. pick() asks for the image first, outside that line: requests
 * made while the dialog is open run meanwhile, and the chosen file's switch is queued like any other.
 */
Singleton {
    id: root

    property var queuedRequest: null
    property bool awaitingThemeRead: false
    readonly property bool busy: proc.running || root.queuedRequest !== null || root.awaitingThemeRead
    readonly property bool picking: picker.running

    signal settled()

    // A request names what the run is asked for, each part optional: `image` (the wallpaper to
    // switch to), `mode` ("dark" or "light"), `schemeType`, the accent `color` (or `clearColor`,
    // or `pickColor` to pick it on the screen), `noSwitch` (new colors for the wallpaper in use).
    function run(request) {
        root.awaitingThemeRead = false;
        if (proc.running) {
            root.queuedRequest = request;
            return;
        }
        root.start(request);
    }

    // The image the user picks, switched to as `request` asks.
    function pick(request = {}) {
        if (picker.running)
            return;
        picker.request = request;
        picker.running = true;
    }

    // The waiting request is dropped only once its run is going, so busy stays true in between.
    function start(request) {
        proc.image = request.image ?? "";
        proc.mode = request.mode ?? "";
        proc.schemeType = request.schemeType ?? "";
        proc.color = request.color ?? "";
        proc.clearColor = request.clearColor ?? false;
        proc.pickColor = request.pickColor ?? false;
        proc.noSwitch = request.noSwitch ?? false;
        proc.running = true;
        root.queuedRequest = null;
    }

    WallpaperPicker {
        id: picker
        property var request: ({})
        stdout: StdioCollector {
            onStreamFinished: {
                const path = text.replace(/\n+$/, "");
                if (path.length > 0)
                    root.run(Object.assign({}, picker.request, { image: path }));
            }
        }
    }

    WallpaperSwitch {
        id: proc
        stderr: StdioCollector {
            id: procErrors
        }
        onExited: (exitCode, exitStatus) => {
            if (exitCode !== 0) {
                const reason = procErrors.text.trim();
                console.warn(`[Switchwall] Wallpaper switch failed with exit code ${exitCode}` + (reason.length > 0 ? `: ${reason}` : ""));
                Notify.send(Translation.tr("Wallpaper switch failed"), reason);
            }
            if (root.queuedRequest !== null) {
                root.start(root.queuedRequest);
                return;
            }
            root.awaitingThemeRead = true;
            MaterialThemeLoader.reapplyTheme();
        }
    }

    Connections {
        target: MaterialThemeLoader
        function onThemeRead() {
            if (!root.awaitingThemeRead)
                return;
            root.awaitingThemeRead = false;
            root.settled();
        }
    }
}
