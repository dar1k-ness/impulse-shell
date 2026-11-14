import QtQuick
import Quickshell

ShellRoot {
    id: root
    property string log: ""
    function row(name, url) {
        log += name + " " + url + "\n"
    }
    function report() {
        row("Text", t.baseUrl)
        row("TextEdit", e.baseUrl)
        row("Loader", loader.item.t.baseUrl)
        t.baseUrl = "x.png"
        row("set", t.baseUrl)
        t.baseUrl = undefined
        row("reset", t.baseUrl)
        return log
    }
    Text { id: t }
    TextEdit { id: e }
    Loader { id: loader; source: "sub/Loaded.qml" }
    Timer { interval: 300; running: true; onTriggered: { console.log("\n" + root.report()); Qt.quit() } }
}
