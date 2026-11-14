pragma Singleton

import ri.modules.common
import ri.modules.common.functions
import QtQuick
import Rustshell
import Rustshell.Io
import Impulse

Singleton {
    id: root

    property string killDialogQmlPath: FileUtils.trimFileProtocol(Rustshell.shellPath("killDialog.qml"))

    function load() {
        // dummy to force init
    }

    readonly property list<string> trays: ["kded6"]
    readonly property list<string> notificationDaemons: ["mako", "dunst"]
    property int query: -1

    Connections {
        target: Config
        function onReadyChanged() {
            if (Config.ready) root.query = Processes.query([...root.trays, ...root.notificationDaemons])
        }
    }

    Connections {
        target: Processes
        function onAnswered(id, running) {
            if (id !== root.query) return;
            const conflictingTrays = root.trays.some(name => running.includes(name));
            const conflictingNotifications = root.notificationDaemons.some(name => running.includes(name));
            var openDialog = false;
            if (conflictingTrays) {
                if (!Config.options.conflictKiller.autoKillTrays) openDialog = true;
                else Processes.terminate(root.trays)
            }
            if (conflictingNotifications) {
                if (!Config.options.conflictKiller.autoKillNotificationDaemons) openDialog = true;
                else Processes.terminate(root.notificationDaemons)
            }
            if (openDialog) {
                ShellControl.openWindow(root.killDialogQmlPath)
            }
        }
    }
}
