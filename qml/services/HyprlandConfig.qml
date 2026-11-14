pragma Singleton
pragma ComponentBehavior: Bound

import QtQuick
import Rustshell
import Rustshell.Hyprland
import Impulse

import ri.modules.common
import ri.modules.common.functions

/**
 * Configs Hyprland
 */
Singleton {
    id: root
    
    signal reloaded()

    readonly property string shellOverridesPath: FileUtils.trimFileProtocol(`${Directories.config}/hypr/hyprland/shellOverrides/main.lua`)

    HyprlandConfigFile {
        id: shellOverrides
        path: root.shellOverridesPath
    }

    function set(key: string, value: var) {
        shellOverrides.set({ [key]: value })
    }

    function setMany(entries: var) {
        shellOverrides.set(entries)
    }

    function reset(key: string) {
        shellOverrides.reset([key])
    }

    function resetMany(keys: list<string>) {
        shellOverrides.reset(keys)
    }

    Connections {
        target: Hyprland

        function onRawEvent(event) {
            if (event.name == "configreloaded") {
                root.reloaded()
            }
        }
    }
}
