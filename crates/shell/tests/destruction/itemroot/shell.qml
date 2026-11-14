import QtQuick
Item {
    Component.onDestruction: console.log("destroyed root")
    Item { Component.onDestruction: console.log("destroyed child") }
    Timer { interval: 60; running: true; onTriggered: { console.log("quit"); Qt.quit() } }
}
