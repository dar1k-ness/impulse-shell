import QtQuick as QQ

QQ.Item {
    id: root
    property int fired: 0
    QQ.Timer {
        id: once
        interval: 10
        running: true
        onTriggered: {
            root.fired++
            console.log("once", root.fired, running, interval, repeat, triggeredOnStart)
            twice.start()
        }
    }
    QQ.Timer {
        id: twice
        interval: 5
        repeat: true
        onTriggered: {
            root.fired++
            console.log("twice", root.fired, running)
            if (root.fired === 3) {
                stop()
                console.log("stopped", running)
                idle.restart()
            }
        }
    }
    QQ.Timer {
        id: idle
        onTriggered: console.log("idle fired")
    }
    QQ.Timer {
        interval: 60
        running: true
        onTriggered: {
            console.log("end", root.fired, idle.running, idle.interval, String(idle).split("(")[0])
            Qt.quit()
        }
    }
    QQ.Component.onCompleted: console.log("start", once.running, twice.running, idle.running, typeof once.start, typeof once.restart)
}
