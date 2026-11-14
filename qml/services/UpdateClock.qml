pragma Singleton

import ri.modules.common
import QtQuick
import Rustshell

/**
 * The one Timer behind every UpdateTimer: all of them tick in the same wakeup of the shell,
 * once per Config.options.resources.updateInterval, and it runs only while one of them does.
 */
Singleton {
    id: root

    // Running UpdateTimers.
    property int users: 0
    signal tick()

    Timer {
        interval: Config.options?.resources?.updateInterval ?? 3000
        running: root.users > 0
        repeat: true
        onTriggered: root.tick()
    }
}
