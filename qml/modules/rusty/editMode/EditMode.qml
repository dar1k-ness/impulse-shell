import QtQuick
import Rustshell
import Rustshell.Hyprland
import Rustshell.Io
import ri
import ri.modules.common
import Impulse

/**
 * KDE-style edit mode (impulse-shell only: the family loads this file only when the layout has
 * containers, so `qs -c impulse-shell` never parses it). Super+Alt+E (the `editModeToggle` global
 * shortcut) or `impulse-shell ipc call editMode toggle` enters it; Esc or Done leaves it. While
 * editing, every monitor without a fullscreen window gets a fullscreen overlay above the
 * shell's surfaces; the native EditController decides everything, the overlay draws.
 */
Scope {
    id: root

    EditController {
        id: controller
    }

    IpcHandler {
        target: "editMode"

        function toggle(): void {
            controller.toggle();
        }
        function start(): void {
            controller.start();
        }
        function finish(): void {
            controller.finish();
        }
    }

    GlobalShortcut {
        name: "editModeToggle"
        description: "Toggles the layout edit mode"
        onPressed: controller.toggle()
    }

    Variants {
        model: Rustshell.screens

        Scope {
            id: monitorScope
            required property var modelData
            property HyprlandMonitor monitor: Hyprland.monitorFor(modelData)
            // A fullscreen window keeps its monitor: no overlay there.
            property list<HyprlandWorkspace> workspacesForMonitor: Hyprland.workspaces.values.filter(workspace => workspace.monitor && workspace.monitor.name == monitor.name)
            property var activeWorkspaceWithFullscreen: workspacesForMonitor.filter(workspace => ((workspace.toplevels.values.filter(window => window.wayland?.fullscreen)[0] != undefined) && workspace.active))[0]
            property bool fullscreen: activeWorkspaceWithFullscreen != undefined

            LazyLoader {
                active: controller.active && !monitorScope.fullscreen
                component: EditOverlay {
                    screen: monitorScope.modelData
                }
            }
        }
    }
}
