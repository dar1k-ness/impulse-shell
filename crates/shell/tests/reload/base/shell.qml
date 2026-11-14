import QtQuick
import Quickshell
import Quickshell.Io

ShellRoot {
    id: root
    Component.onCompleted: console.log("ready one")
    Component.onDestruction: console.log("destroyed root")
    Part {}
    Connections {
        target: Quickshell
        function onReloadCompleted() {
            console.log("reloadCompleted");
        }
        function onReloadFailed(error) {
            console.log("reloadFailed " + JSON.stringify(error));
        }
    }
    IpcHandler {
        target: "t"
        function reload(): void {
            Quickshell.reload(true);
        }
        function alive(): void {
            console.log("alive");
        }
    }
}
