pragma Singleton

import ri.modules.common
import ri.modules.common.functions
import QtQuick
import Rustshell
import Rustshell.Io
import Impulse

/*
 * System updates service. Currently only supports Arch.
 */
Singleton {
    id: root

    property bool available: false
    property alias checking: checkUpdatesProc.running
    property int count: 0
    
    readonly property bool updateAdvised: available && count > Config.options.updates.adviseUpdateThreshold
    readonly property bool updateStronglyAdvised: available && count > Config.options.updates.stronglyAdviseUpdateThreshold

    function load() {}
    function refresh() {
        if (!available) return;
        print("[Updates] Checking for system updates")
        checkUpdatesProc.running = true;
    }

    Timer {
        interval: Config.options.updates.checkInterval * 60 * 1000
        repeat: true
        running: Config.ready && Config.options.updates.enableCheck
        onTriggered: {
            print("[Updates] Periodic update check due")
            root.refresh();
        }
    }

    UpdatesProbe {
        id: checkAvailabilityProc
        running: Config.ready && Config.options.updates.enableCheck
        onExited: (exitCode, exitStatus) => {
            root.available = (exitCode === 0);
            root.refresh();
        }
    }

    UpdatesCount {
        id: checkUpdatesProc
        stdout: StdioCollector {
            onStreamFinished: {
                root.count = parseInt(text.trim());
            }
        }
    }
}
