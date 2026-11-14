import QtQuick
Item {
    id: r
    Component.onDestruction: console.log("root onDestruction")
    Component.onCompleted: { try { r.destroy(); console.log("no throw") } catch (e) { console.log("threw:", e.message) } }
    Timer { interval: 50; running: true; onTriggered: console.log("root still alive at 50ms") }
}
