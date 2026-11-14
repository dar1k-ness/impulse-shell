pragma Singleton
pragma ComponentBehavior: Bound

import QtQuick
import Rustshell
import Rustshell.Io
import Rustshell.Wayland
import Rustshell.Hyprland
import Impulse

/**
 * Provides access to some Hyprland data not available in Rustshell.Hyprland.
 */
Singleton {
    id: root
    property var windowList: []
    property var addresses: []
    property var windowByAddress: ({})
    property var workspaces: []
    property var workspaceIds: []
    property var workspaceById: ({})
    property var activeWorkspace: null
    property var monitors: []
    property var layers: ({})

    // Convenient stuff

    function toplevelsForWorkspace(workspace) {
        return ToplevelManager.toplevels.values.filter(toplevel => {
            const address = `0x${toplevel.HyprlandToplevel?.address}`;
            var win = HyprlandData.windowByAddress[address];
            return win?.workspace?.id === workspace;
        })
    }

    function hyprlandClientsForWorkspace(workspace) {
        return root.windowList.filter(win => win.workspace.id === workspace);
    }

    function clientForToplevel(toplevel) {
        if (!toplevel || !toplevel.HyprlandToplevel) {
            return null;
        }
        const address = `0x${toplevel?.HyprlandToplevel?.address}`;
        return root.windowByAddress[address];
    }

    // Internals

    function updateWindowList() {
        getClients.send();
    }

    function updateLayers() {
        getLayers.send();
    }

    function updateMonitors() {
        getMonitors.send();
    }

    function updateWorkspaces() {
        getWorkspaces.send();
        getActiveWorkspace.send();
    }

    function updateAll() {
        updateWindowList();
        updateMonitors();
        updateLayers();
        updateWorkspaces();
    }

    function biggestWindowForWorkspace(workspaceId) {
        const windowsInThisWorkspace = HyprlandData.windowList.filter(w => w.workspace.id == workspaceId);
        return windowsInThisWorkspace.reduce((maxWin, win) => {
            const maxArea = (maxWin?.size?.[0] ?? 0) * (maxWin?.size?.[1] ?? 0);
            const winArea = (win?.size?.[0] ?? 0) * (win?.size?.[1] ?? 0);
            return winArea > maxArea ? win : maxWin;
        }, null);
    }

    Component.onCompleted: {
        updateAll();
    }

    // What a Hyprland event can change. Events not listed refresh everything.
    readonly property var eventQueries: ({
        "activelayout": [], "submap": [], "bell": [],
        // No event reports a window moved or resized by hand. The overview's window previews
        // start a screencast when it opens: that is when the windows are read again.
        "screencast": ["clients", "workspaces"],
        // `monitors` too: their `reserved` area follows the layers' exclusive zones.
        "openlayer": ["layers", "monitors"], "closelayer": ["layers", "monitors"],
        "workspace": ["monitors", "workspaces"], "focusedmon": ["monitors", "workspaces"],
        "activespecial": ["monitors", "workspaces"],
        "activewindow": ["clients", "workspaces"], "windowtitle": ["clients", "workspaces"],
        "openwindow": ["clients", "workspaces"], "closewindow": ["clients", "workspaces"],
        "movewindow": ["clients", "workspaces"], "changefloatingmode": ["clients", "workspaces"],
        "fullscreen": ["clients", "workspaces"], "pin": ["clients", "workspaces"],
        "minimized": ["clients", "workspaces"], "urgent": ["clients", "workspaces"],
        "togglegroup": ["clients", "workspaces"], "moveintogroup": ["clients", "workspaces"],
        "moveoutofgroup": ["clients", "workspaces"], "lockgroups": ["clients", "workspaces"],
        "ignoregrouplock": ["clients", "workspaces"],
    })

    // The reply of a request as JSON; a reply that is not JSON (Hyprland is not running) is
    // logged and leaves the data as it was.
    function parseReply(text) {
        try {
            return JSON.parse(text);
        } catch (e) {
            console.log(`[HyprlandData] ${e}: ${text.trim()}`);
            return undefined;
        }
    }

    Connections {
        target: Hyprland

        function onRawEvent(event) {
            // console.log("Hyprland raw event:", event.name);
            // `screencastv2` follows it. A screencast that stopped changed nothing: Hyprland
            // reports that half a second after the last copied frame, so reading the windows
            // again then (a repaint, a frame, the next start) would never end.
            if (event.name === "screencast") return;
            if (event.name === "screencastv2" && !event.data.startsWith("1,")) return;
            const name = event.name.endsWith("v2") ? event.name.slice(0, -2) : event.name;
            const queries = root.eventQueries[name];
            if (queries === undefined) {
                root.updateAll();
                return;
            }
            if (queries.includes("clients")) root.updateWindowList();
            if (queries.includes("monitors")) root.updateMonitors();
            if (queries.includes("layers")) root.updateLayers();
            if (queries.includes("workspaces")) root.updateWorkspaces();
        }
    }

    HyprlandRequest {
        id: getClients
        command: "clients"
        json: true
        onReplied: text => {
            const clients = root.parseReply(text);
            if (clients === undefined) return;
            root.windowList = clients
            let tempWinByAddress = {};
            for (var i = 0; i < root.windowList.length; ++i) {
                var win = root.windowList[i];
                tempWinByAddress[win.address] = win;
            }
            root.windowByAddress = tempWinByAddress;
            root.addresses = root.windowList.map(win => win.address);
        }
    }

    HyprlandRequest {
        id: getMonitors
        command: "monitors"
        json: true
        onReplied: text => {
            const monitors = root.parseReply(text);
            if (monitors === undefined) return;
            root.monitors = monitors;
        }
    }

    HyprlandRequest {
        id: getLayers
        command: "layers"
        json: true
        onReplied: text => {
            const layers = root.parseReply(text);
            if (layers === undefined) return;
            root.layers = layers;
        }
    }

    HyprlandRequest {
        id: getWorkspaces
        command: "workspaces"
        json: true
        onReplied: text => {
            var rawWorkspaces = root.parseReply(text);
            if (rawWorkspaces === undefined) return;
            // Filter out invalid workspace ids (e.g. lock-screen temp workspace 2147483647 - N)
            root.workspaces = rawWorkspaces.filter(ws => ws.id >= 1 && ws.id <= 100);
            let tempWorkspaceById = {};
            for (var i = 0; i < root.workspaces.length; ++i) {
                var ws = root.workspaces[i];
                tempWorkspaceById[ws.id] = ws;
            }
            root.workspaceById = tempWorkspaceById;
            root.workspaceIds = root.workspaces.map(ws => ws.id);
        }
    }

    HyprlandRequest {
        id: getActiveWorkspace
        command: "activeworkspace"
        json: true
        onReplied: text => {
            const workspace = root.parseReply(text);
            if (workspace === undefined) return;
            root.activeWorkspace = workspace;
        }
    }
}
