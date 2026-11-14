pragma Singleton

import QtQuick
import Rustshell

/**
 * Index of the settings app rows for its search field. Rows register themselves; a search reads
 * their current title/subtitle, so translations and dynamic texts stay right.
 */
Singleton {
    id: root
    property var entries: []

    function register(item) {
        root.entries.push(item);
    }

    function unregister(item) {
        const i = root.entries.indexOf(item);
        if (i !== -1)
            root.entries.splice(i, 1);
    }

    // Group and tab titles of a row, so "lock screen" or "sound" finds every row filed there.
    function context(item) {
        let out = "";
        for (let it = item.parent; it; it = it.parent) {
            if (it.settingsGroup === true && it.title.length > 0)
                out += ` ${it.title}`;
            if (it.settingsTab === true)
                out += ` ${it.tabName}`;
            if (it.settingsPage === true)
                break;
        }
        return out;
    }

    // Matches on every word; rows whose own title matches come first, title prefixes before that.
    function search(query) {
        const q = query.trim().toLowerCase();
        if (q.length === 0)
            return [];
        const words = q.split(/\s+/);
        const ranked = [];
        for (let i = 0; i < root.entries.length; i++) {
            const e = root.entries[i];
            const title = `${e.title}`.toLowerCase();
            const hay = `${title} ${e.subtitle} ${e.keywords} ${root.context(e)}`.toLowerCase();
            if (!words.every(w => hay.indexOf(w) !== -1))
                continue;
            let rank = 3;
            if (title.startsWith(q))
                rank = 0;
            else if (title.indexOf(q) !== -1)
                rank = 1;
            else if (words.every(w => title.indexOf(w) !== -1))
                rank = 2;
            ranked.push({ rank: rank, order: i, entry: e });
        }
        ranked.sort((a, b) => a.rank - b.rank || a.order - b.order);
        return ranked.map(r => r.entry);
    }
}
