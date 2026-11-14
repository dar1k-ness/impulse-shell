pragma Singleton
pragma ComponentBehavior: Bound

import ri.modules.common
import ri.modules.common.functions
import QtQuick
import Rustshell
import Rustshell.Io
import Rustshell.Hyprland
import Impulse

/**
 * A service that provides access to Hyprland keybinds.
 * Asks Hyprland for its binds (`binds` request, JSON) at startup and after each config reload.
 */
Singleton {
    id: root
    property var keybinds: []
    property var keybindCategories: []

    Connections {
        target: Hyprland

        function onRawEvent(event) {
            if (event.name == "configreloaded") {
                getKeybinds.send()
            }
        }
    }

    Component.onCompleted: getKeybinds.send()

    HyprlandRequest {
        id: getKeybinds
        command: "binds"
        json: true

        onReplied: text => {
            try {
                root.keybinds = JSON.parse(text)
                var groups = []
                for (var i = 0; i < root.keybinds.length; i++) {
                    var bind = root.keybinds[i].description
                    var group = bind.substring(0, bind.indexOf(":"))
                    if (!groups.includes(group) && group.length > 0) {
                        groups.push(group)
                    }
                }
                root.keybindCategories = groups
            } catch (e) {
                console.error("[CheatsheetKeybinds] Error parsing keybinds:", e)
            }
        }
    }
}

