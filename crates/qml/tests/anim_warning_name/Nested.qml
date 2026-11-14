import QtQuick
SequentialAnimation {
    id: root
    NumberAnimation { id: inner; duration: 1000 }
    property Timer t: Timer {
        interval: 50; running: true
        onTriggered: {
            inner.start()
            root.pause()
            Qt.quit()
        }
    }
}
