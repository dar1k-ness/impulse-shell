import QtQuick as QQ

QQ.Item {
    id: part
    enum Mode {
        Slow,
        Fast
    }
    property int n: 0
    property int ticks: 0
    signal pinged(int k)
    default property alias content: holder.data
    component Inner: QQ.Rectangle {
        property int deep: 3
    }
    function ping() {
        pinged(n)
    }
    QQ.Item {
        id: holder
    }
    QQ.Timer {
        interval: 5
        running: part.n > 0
        onTriggered: part.ticks++
    }
}
