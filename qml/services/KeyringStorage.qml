pragma Singleton
pragma ComponentBehavior: Bound

import ri
import ri.modules.common
import ri.modules.common.functions
import Impulse
import Rustshell;
import Rustshell.Io;
import QtQuick;

/**
 * For storing sensitive data in the keyring.
 * Use this for small data only, since it stores a JSON of the contents directly and doesn't use a database.
 */
Singleton {
    id: root

    signal dataChanged()

    property bool loaded: false
    property var keyringData: ({})
    // Fields set before the stored data was read, as [path, value]. Storing then would replace
    // every saved key with them: they wait for the read and go on top of what it returns.
    property var pendingFields: []
    
    property var properties: {
        "application": "impulse-shell",
        "explanation": Translation.tr("For storing API keys and other sensitive information"),
    }
    property string keyringLabel: Translation.tr("%1 Safe Storage").arg("impulse-shell")

    function setNestedField(path, value) {
        root.assignNestedField(path, value);
        if (!root.loaded) root.pendingFields.push([path, value]);
        saveKeyringData();
    }

    function assignNestedField(path, value) {
        if (!root.keyringData) root.keyringData = {};
        let keys = path;
        let obj = root.keyringData;
        let parents = [obj];

        // Traverse and collect parent objects
        for (let i = 0; i < keys.length - 1; ++i) {
            if (!obj[keys[i]] || typeof obj[keys[i]] !== "object") {
                obj[keys[i]] = {};
            }
            obj = obj[keys[i]];
            parents.push(obj);
        }

        // Set the value at the innermost key
        obj[keys[keys.length - 1]] = value;

        // Reassign each parent object from the bottom up to trigger change notifications
        for (let i = keys.length - 2; i >= 0; --i) {
            let parent = parents[i];
            let key = keys[i];
            // Shallow clone to change object identity (spread replaced with Object.assign)
            parent[key] = Object.assign({}, parent[key]);
        }

        // Finally, reassign root.keyringData to trigger top-level change
        root.keyringData = Object.assign({}, root.keyringData);
    }

    function fetchKeyringData() {
        Keyring.lookup({ "application": root.properties.application });
    }

    function saveKeyringData() {
        if (!root.loaded) {
            // Not read yet: read first (unlocking the keyring this write needs), the lookup's
            // answer stores.
            Keyring.lookup({ "application": root.properties.application }, true);
            return;
        }
        Keyring.store(root.keyringLabel, root.properties, JSON.stringify(root.keyringData));
        root.dataChanged()
    }

    Connections {
        target: Keyring
        function onLookupFinished(result, secret) {
            if (result === Keyring.Locked) return;
            let save = root.pendingFields.length > 0;
            if (result === Keyring.NotFound) {
                console.error("[KeyringStorage] Entry not found, initializing.");
                root.keyringData = {};
                save = true;
            } else if (secret.startsWith("{")) {
                try {
                    root.keyringData = JSON.parse(secret);
                } catch (e) {
                    console.error("[KeyringStorage] Failed to get keyring data, reinitializing.");
                    root.keyringData = {};
                    save = true;
                }
            }
            const pending = root.pendingFields;
            root.pendingFields = [];
            for (let i = 0; i < pending.length; ++i) {
                root.assignNestedField(pending[i][0], pending[i][1]);
            }
            root.loaded = true;
            if (save) root.saveKeyringData();
        }
    }
}
