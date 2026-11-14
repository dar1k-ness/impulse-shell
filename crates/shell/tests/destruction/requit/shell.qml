import QtQuick
QtObject {
    id: root
    Component.onDestruction: console.log("destroyed root")
    property list<QtObject> data: [
        QtObject {
            Component.onDestruction: {
                console.log("destroyed a, quits")
                Qt.quit()
                console.log("a after quit")
            }
        },
        QtObject {
            Component.onDestruction: {
                console.log("destroyed b, exits with 5")
                Qt.exit(5)
                console.log("b after exit")
            }
        },
        Timer {
            interval: 60; running: true
            onTriggered: {
                console.log("exit with 3")
                Qt.exit(3)
                console.log("quit again")
                Qt.quit()
                console.log("after both")
            }
        }
    ]
}
