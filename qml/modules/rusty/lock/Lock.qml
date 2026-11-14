pragma ComponentBehavior: Bound
import ri
import ri.services
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.panels.lock
import QtQuick
import Rustshell
import Rustshell.Hyprland

LockScreen {
    id: root

    // Monitor name -> workspace id to restore on unlock (set when locking)
    property var savedWorkspaces: ({})

    Timer {
        id: restoreTimer
        interval: 150
        repeat: false
        onTriggered: {
            for (var j = 0; j < Rustshell.screens.length; ++j) {
                var monName = Rustshell.screens[j].name
                var wsId = root.savedWorkspaces[monName]
                if (wsId !== undefined) {
                    root.showWorkspace(monName, wsId)
                }
            }
            root.savedWorkspaces = ({})
        }
    }

    // A monitor that reconnects while locked (HDMI waking from sleep) comes back on a
    // real workspace, whose windows show through the transparent lock (session_lock_xray).
    function rehideWorkspaces() {
        for (var i = 0; i < Rustshell.screens.length; ++i) {
            var mon = Rustshell.screens[i].name
            var mData = HyprlandData.monitors.find(m => m.name === mon)
            var ws = mData?.activeWorkspace?.id
            if (ws === undefined) {
                continue;
            }
            if (root.savedWorkspaces[mon] === undefined) {
                root.savedWorkspaces[mon] = ws
            }
            var target = 2147483647 - root.savedWorkspaces[mon]
            if (ws !== target) {
                root.showWorkspace(mon, target)
            }
        }
    }

    // Focus `monitor`, then switch it to `workspace`. Hyprland takes the requests in the order
    // they are made, so a lock or unlock over several monitors does not race itself.
    function showWorkspace(monitor, workspace) {
        Hyprland.dispatch(`hl.dsp.focus({monitor=${root.luaString(monitor)}})`)
        Hyprland.dispatch(`hl.dsp.focus({workspace=${root.luaWorkspace(workspace)}})`)
    }

    // `text` as a Lua string literal
    function luaString(text) {
        return '"' + String(text).replace(/[\\"\x00-\x1f\x7f]/g, c => {
            if (c === "\\" || c === '"') return "\\" + c;
            return "\\" + String(c.charCodeAt(0)).padStart(3, "0");
        }) + '"';
    }

    // A workspace id as a Lua number; anything else as a Lua string, never as Lua code
    function luaWorkspace(workspace) {
        return Number.isInteger(workspace) ? String(workspace) : root.luaString(workspace);
    }

    Connections {
        target: HyprlandData
        function onMonitorsChanged() {
            // HyprlandData.monitors now holds Hyprland's reply: each monitor's active workspace
            if (GlobalStates.screenLocked) {
                root.rehideWorkspaces()
            }
        }
    }

    lockSurface: LockSurface {
        context: root.context
    }

    Connections {
        target: GlobalStates
        function onScreenLockedChanged() {
            if (GlobalStates.screenLocked) {
                // Lock: save workspace per monitor and move all to temp workspace in one batch
                var next = {}
                var moves = []
                for (var i = 0; i < Rustshell.screens.length; ++i) {
                    var mon = Rustshell.screens[i].name
                    var mData = HyprlandData.monitors.find(m => m.name === mon)
                    if (mData?.activeWorkspace == undefined) {
                        return;
                    }
                    var ws = (mData?.activeWorkspace?.id ?? 1)
                    next[mon] = ws
                    moves.push([mon, 2147483647 - ws])
                }
                root.savedWorkspaces = next
                for (const [monitor, workspace] of moves) {
                    root.showWorkspace(monitor, workspace)
                }
            } else {
                restoreTimer.start()
            }
        }
    }

    // Push everything down (visual only; workspace switch is in Connections above)
    Variants {
        model: Rustshell.screens
        delegate: Scope {
            required property ShellScreen modelData
            property bool shouldPush: GlobalStates.screenLocked
            property string targetMonitorName: modelData.name
            property int verticalMovementDistance: modelData.height
            property int horizontalSqueeze: modelData.width * 0.2
        }
    }
}
