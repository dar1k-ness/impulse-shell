pragma Singleton

import QtQuick
import Impulse
import Rustshell
import Rustshell.Io
import ri.modules.common

/**
 * Wallpaper picker: finds a wallpaper on Wallhaven/Konachan that fits the taste described in
 * Config.options.background.wallpick (a local image model scores the candidates), applies it
 * with a matching accent, and learns from likes and dislikes. The work runs in process
 * (`WallpickRun`); `rusty-impulse wallpick` does the same from a terminal.
 */
Singleton {
    id: root

    readonly property bool busy: proc.running
    // The step being worked on while busy, the outcome of the last run otherwise
    property string status: ""
    property bool failed: false

    // A run of `args` ended, `ok` without an error.
    signal ended(var args, bool ok)

    function pick() {
        root.run(["pick"]);
    }
    function like() {
        root.run(["like"]);
    }
    function dislike() {
        root.run(["dislike", "--next"]);
    }
    function restyle() {
        root.run(["style"]);
    }
    // the original's bar groups per part of the bar, as bar.layout holds them.
    function arrange(left, center, right) {
        root.run(["arrange", left.join(","), center.join(","), right.join(",")]);
    }
    function resetTaste() {
        root.run(["reset-taste"]);
    }

    // A run of `args` ended. One that put a new wallpaper and style on the screen ("pick",
    // "dislike --next") asks the shell to show them for review (services/StyleReview.qml):
    // from the settings window that is another process, hence the IPC call.
    function finished(args, exitCode) {
        root.failed = exitCode !== 0;
        root.ended(args, !root.failed);
        if (!root.failed && (args[0] === "pick" || args[1] === "--next"))
            ShellControl.ipcCall("styleReview", "open");
    }

    function run(args) {
        if (proc.running)
            return;
        root.status = "";
        root.failed = false;
        proc.args = args;
        proc.running = true;
    }

    WallpickRun {
        id: proc
        stdout: SplitParser {
            onRead: data => root.status = data
        }
        stderr: SplitParser {
            onRead: data => root.status = data
        }
        onExited: (exitCode, exitStatus) => root.finished(proc.args, exitCode)
    }
}
