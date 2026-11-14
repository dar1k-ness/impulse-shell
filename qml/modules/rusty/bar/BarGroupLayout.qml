pragma Singleton
pragma ComponentBehavior: Bound
import QtQuick
import Rustshell
import ri.modules.common

// Which of the original's bar groups each part of the bar holds, in order, from Config.options.bar.layout.
// Unknown names and repeats are dropped, and a group no part names goes back to its own place
// in the center, so a missing or broken layout is the original's own: media, workspaces, clock.
Singleton {
    id: root

    readonly property list<string> groups: ["media", "workspaces", "clock"]
    readonly property var parts: {
        const layout = Config.options?.bar?.layout;
        const placed = [];
        const pick = names => {
            const kept = [];
            const count = typeof names === "object" ? (names?.length ?? 0) : 0;
            for (let i = 0; i < count; i++) {
                const name = names[i];
                if (root.groups.indexOf(name) !== -1 && placed.indexOf(name) === -1) {
                    placed.push(name);
                    kept.push(name);
                }
            }
            return kept;
        };
        const left = pick(layout?.left);
        const center = pick(layout?.center);
        const right = pick(layout?.right);
        for (let i = 0; i < root.groups.length; i++) {
            const name = root.groups[i];
            if (placed.indexOf(name) !== -1)
                continue;
            // Before the first placed center group that comes after it in the original's order.
            let at = center.length;
            for (let j = 0; j < center.length; j++) {
                if (root.groups.indexOf(center[j]) > i) {
                    at = j;
                    break;
                }
            }
            center.splice(at, 0, name);
        }
        return { left: left, center: center, right: right };
    }
    readonly property list<string> left: parts.left
    readonly property list<string> center: parts.center
    readonly property list<string> right: parts.right
}
