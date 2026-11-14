pragma Singleton

import QtQuick
import Rustshell
import Rustshell.Io
import Rustshell.Hyprland
import Impulse
import ri.modules.common

/**
 * Exposes the active Hyprland Xkb keyboard layout name and code for indicators.
 */
Singleton {
    id: root
    // You can read these
    property list<string> layoutCodes: []
    property var cachedLayoutCodes: ({})
    property string currentLayoutName: ""
    property string currentLayoutCode: ""
    // For the service
    property var baseLayoutFilePath: "/usr/share/X11/xkb/rules/base.lst"
    property bool needsLayoutRefresh: false

    // Update the layout code according to the layout name (Hyprland gives the name not the code)
    onCurrentLayoutNameChanged: root.updateLayoutCode()
    function updateLayoutCode() {
        if (cachedLayoutCodes.hasOwnProperty(currentLayoutName)) {
            root.currentLayoutCode = cachedLayoutCodes[currentLayoutName];
        } else if (baseLayoutFile.loaded) {
            root.findLayoutCode(baseLayoutFile.text());
        } else {
            // Read on the first lookup; onLoaded finishes it
            baseLayoutFile.path = root.baseLayoutFilePath;
        }
    }

    // Get the layout code from the base.lst file by grabbing the line with the current layout name
    FileView {
        id: baseLayoutFile
        onLoaded: root.findLayoutCode(baseLayoutFile.text())
    }

    function findLayoutCode(baseList) {
        const lines = baseList.split("\n");
        const targetDescription = root.currentLayoutName;
        const foundLine = lines.find(line => {
            // Skip comment lines and empty lines
            if (!line.trim() || line.trim().startsWith('!'))
                return false;

            // Match layout: (whitespace + ) key + whitespace + description
            const matchLayout = line.match(/^\s*(\S+)\s+(.+)$/);
            if (matchLayout && matchLayout[2] === targetDescription) {
                root.cachedLayoutCodes[matchLayout[2]] = matchLayout[1];
                root.currentLayoutCode = matchLayout[1];
                return true;
            }

            // Match variant: (whitespace + ) variant + whitespace + key + whitespace + description
            const matchVariant = line.match(/^\s*(\S+)\s+(\S+)\s+(.+)$/);
            if (matchVariant && matchVariant[3] === targetDescription) {
                const complexLayout = matchVariant[2] + matchVariant[1];
                root.cachedLayoutCodes[matchVariant[3]] = complexLayout;
                root.currentLayoutCode = complexLayout;
                return true;
            }

            return false;
        });
        // console.log("[HyprlandXkb] Found line:", foundLine);
        // console.log("[HyprlandXkb] Layout:", root.currentLayoutName, "| Code:", root.currentLayoutCode);
        // console.log("[HyprlandXkb] Cached layout codes:", JSON.stringify(root.cachedLayoutCodes, null, 2));
    }

    // A keyboard made by a client (ours for the on-screen keyboard and paste keys, wtype, an
    // input method), not one the user types on
    function isVirtualKeyboard(name) {
        return name === "hl-virtual-keyboard" || name.startsWith("hl-virtual-keyboard-");
    }

    // Find out available layouts and current active layout. Should only be necessary on init
    Component.onCompleted: fetchLayouts.send()
    HyprlandRequest {
        id: fetchLayouts
        command: "devices"
        json: true
        onReplied: text => {
            let parsedOutput;
            try {
                parsedOutput = JSON.parse(text);
            } catch (e) {
                console.log(`[HyprlandXkb] ${e}: ${text.trim()}`);
                return;
            }
            const keyboards = parsedOutput["keyboards"];
            const isReal = kb => !root.isVirtualKeyboard(kb.name);
            // `main` is the keyboard that sent the last key, which is a virtual keyboard after an
            // on-screen key or a paste: its layout is that client's, not the one the user types in
            const hyprlandKeyboard = keyboards.find(kb => kb.main === true && isReal(kb))
                ?? keyboards.find(isReal)
                ?? keyboards.find(kb => kb.main === true);
            if (hyprlandKeyboard === undefined) return;
            root.layoutCodes = hyprlandKeyboard["layout"].split(",");
            root.currentLayoutName = hyprlandKeyboard["active_keymap"];
            // console.log("[HyprlandXkb] Fetched | Layouts (multiple: " + (root.layoutCodes.length > 1) + "): "
            //     + root.layoutCodes.join(", ") + " | Active: " + root.currentLayoutName);
        }
    }

    // Update the layout name when it changes
    Connections {
        target: Hyprland
        function onRawEvent(event) {
            if (event.name === "activelayout") {
                if (root.needsLayoutRefresh) {
                    root.needsLayoutRefresh = false;
                    fetchLayouts.send();
                }

                // If there's only one layout, the updated layout is always the same
                if (root.layoutCodes.length <= 1) return;

                // Update when layout might have changed
                const dataString = event.data;
                // The event comes per keyboard (KEYBOARD,LAYOUT). A virtual keyboard keeps its own
                // layout, which can be the other one
                if (dataString.includes(",") && root.isVirtualKeyboard(dataString.split(",")[0])) return;
                root.currentLayoutName = dataString.substring(dataString.indexOf(",") + 1);

                // Update layout for on-screen keyboard (osk)
                Config.options.osk.layout = root.currentLayoutName.split(" (")[0];
            } else if (event.name == "configreloaded") {
                // Mark layout code list to be updated when config is reloaded
                root.needsLayoutRefresh = true;
            }
        }
    }
}
