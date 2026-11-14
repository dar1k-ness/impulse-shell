import QtQuick
import "." as My
Item {
    id: root
    property real v: 0
    My.Idle { id: alone }
    SequentialAnimation {
        id: group
        My.Idle { id: inner; duration: 1000 }
    }
    Behavior on v {
        My.Idle { id: led; duration: 1000 }
    }
    Timer {
        interval: 50; running: true
        onTriggered: {
            alone.pause()
            inner.start()
            group.start()
            inner.pause()
            root.v = 1
            led.pause()
            led.stop()
            Qt.quit()
        }
    }
}
