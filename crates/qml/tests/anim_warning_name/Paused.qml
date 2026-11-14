import QtQuick
NumberAnimation {
    id: root
    property Timer t: Timer {
        interval: 50; running: true
        onTriggered: {
            root.pause()
            Qt.quit()
        }
    }
}
