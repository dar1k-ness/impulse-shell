import QtQuick
import QtQuick as QQ
Item {
    Component { id: qualified; QQ.Rectangle { } }
    QtObject { id: plain }
    Component.onCompleted: {
        qualified.createObject(plain)
        var document = Qt.createComponent("Good.qml")
        document.createObject(plain)
        console.log("status " + document.status)
        Qt.quit()
    }
}
