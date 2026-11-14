import QtQuick
import Quickshell

ShellRoot {
    readonly property string apps: DesktopEntries.applications.values.map(e => e.id + ":" + e.name).sort().join(",") || "-"
    onAppsChanged: console.log("EV " + Date.now() + " " + apps)
    Component.onCompleted: console.log("EV " + Date.now() + " " + apps)
}
