import QtQuick
import Quickshell
Scope {
    id: root
    QtObject { Component.onDestruction: console.log("child destroyed") }
    LazyLoader { active: true
        QtObject { Component.onDestruction: console.log("loaded obj destroyed") } }
    Component.onDestruction: console.log("root destroyed")
    Timer { interval: 60; running: true; onTriggered: { console.log("quit"); Qt.quit() } }
}
