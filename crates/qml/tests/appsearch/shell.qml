import QtQuick
import Quickshell
import Quickshell.Io
import qs.services

// Drives the tree's LauncherSearch like the overview search field: `q` sets the query and lists the
// app results with their actions. Loaded with the tree as the config root.
ShellRoot {
    id: root
    function q(s) {
        LauncherSearch.query = s;
        return LauncherSearch.results.filter(x => x && x.id).map(x => x.id + "=" + x.name
            + (x.actions.length ? "/" + x.actions.map(a => a ? a.name : "NULL").join("+") : "")).join(",") || "-";
    }
    IpcHandler {
        target: "t"
        function q(s: string): string { return root.q(s); }
    }
}
