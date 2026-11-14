pragma Singleton
pragma ComponentBehavior: Bound

import ri.modules.common
import ri.services.assistant
import Rustshell
import QtQuick

/**
 * The left sidebar's chat with the agent set in `sidebar.assistant.command` (any agent that
 * speaks the Agent Client Protocol over stdio). The model and mode picked last are kept.
 */
Singleton {
    id: root

    readonly property bool configured: Config.options.sidebar.assistant.command.length > 0

    property AcpSession session: AcpSession {
        command: Config.options.sidebar.assistant.command
        preferredMode: Persistent.states?.assistant?.mode ?? ""
        preferredModel: Persistent.states?.assistant?.model ?? ""
        onModelChosen: model => Persistent.states.assistant.model = model
        onModeChosen: mode => Persistent.states.assistant.mode = mode
    }
}
