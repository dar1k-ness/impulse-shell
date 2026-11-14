pragma Singleton
pragma ComponentBehavior: Bound

import ri.modules.common
import Impulse
import QtQuick
import Rustshell
import Rustshell.Io

Singleton {
    id: root
    property bool sloppySearch: Config.options?.search.sloppy ?? false
    property real scoreThreshold: 0.2
    readonly property list<string> entries: ClipboardHistory.entries
    readonly property var preparedEntries: entries.map(a => ({
        name: FuzzySort.prepare(`${a.replace(/^\s*\S+\s+/, "")}`),
        entry: a
    }))
    function fuzzyQuery(search: string): var {
        if (search.trim() === "") {
            return entries;
        }
        if (root.sloppySearch) {
            const results = entries.slice(0, 100).map(str => ({
                entry: str,
                score: Levenshtein.computeTextMatchScore(str.toLowerCase(), search.toLowerCase())
            })).filter(item => item.score > root.scoreThreshold)
                .sort((a, b) => b.score - a.score)
            return results
                .map(item => item.entry)
        }

        return FuzzySort.go(search, preparedEntries, {
            all: true,
            key: "name"
        }).map(r => {
            return r.obj.entry
        });
    }

    function entryIsImage(entry) {
        return !!(/^\d+\t\[\[.*binary data.*\d+x\d+.*\]\]$/.test(entry))
    }

    // The history is cliphist's; a stash-like tool is set with ClipboardHistory.binary.
    function refresh() {
        ClipboardHistory.refresh()
    }

    function copy(entry) {
        ClipboardHistory.copy(entry)
    }

    function superpaste(count, isImage = false) {
        // Find entries
        const targetEntries = entries.filter(entry => {
            if (!isImage) return true;
            return entryIsImage(entry);
        }).slice(0, count)
        // Act
        ClipboardHistory.superpaste([...targetEntries].reverse())
    }

    function deleteEntry(entry) {
        ClipboardHistory.remove(entry)
    }

    function wipe() {
        ClipboardHistory.wipe()
    }

    Connections {
        target: Rustshell
        function onClipboardTextChanged() {
            // The history lists itself again when its database changes; where the database
            // cannot be watched, list after the delay cliphist needs to store the copy.
            if (!ClipboardHistory.watched())
                delayedUpdateTimer.restart()
        }
    }

    Timer {
        id: delayedUpdateTimer
        interval: Config.options.hacks.arbitraryRaceConditionDelay
        repeat: false
        onTriggered: {
            root.refresh()
        }
    }

    IpcHandler {
        target: "cliphistService"

        function update(): void {
            root.refresh()
        }
    }
}
