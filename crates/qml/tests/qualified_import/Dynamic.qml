import QtQuick as QQ

QQ.Item {
    id: root
    function typedArg(it: QQ.Item): QQ.Item {
        return it
    }
    QQ.Component.onCompleted: {
        var t = Qt.createQmlObject("import QtQuick as Q2\nQ2.Timer { interval: 10; onTriggered: { console.log('dynamic timer', interval); Qt.quit() } }", root)
        console.log("created", String(t).split("(")[0], t.running, t.repeat, typeof t.start, t instanceof QQ.Timer)
        t.start()
        console.log("started", t.running)
        var a = Qt.createQmlObject("import QtQuick as Q2\nQ2.NumberAnimation { duration: 5 }", root)
        console.log("animation", typeof a.pause, a.duration, a.running, a instanceof QQ.NumberAnimation, a instanceof QQ.Animation)
        console.log("typed", typedArg(root) === root)
        try {
            Qt.createQmlObject("import QtQuick as Q2\nQ2.NoSuchType {}", root, "dyn.qml")
        } catch (e) {
            console.log("bad", String(e).split("dyn.qml:")[1])
        }
        try {
            Qt.createQmlObject("import QtQuick\nNoNs.Item {}", root, "dyn.qml")
        } catch (e) {
            console.log("bad", String(e).split("dyn.qml:")[1])
        }
        try {
            Qt.createQmlObject("import QtQuick\nimport QtQuick.Layouts as L\nL.Rectangle {}", root, "dyn.qml")
        } catch (e) {
            console.log("bad", String(e).split("dyn.qml:")[1])
        }
        try {
            Qt.createQmlObject("import QtQuick as Q2\nimport QtQuick.Layouts as L\nQ2.Item { L.Timer {} }", root, "dyn.qml")
        } catch (e) {
            console.log("bad", String(e).split("dyn.qml:")[1])
        }
        try {
            var two = Qt.createQmlObject("import QtQuick as Q2\nimport QtQuick.Layouts as Q2\nQ2.RowLayout { Q2.Timer { id: t } property bool r: t.running; property int i: t.interval }", root, "dyn.qml")
            console.log("shared qualifier", two.r, two.i, String(two).split("_")[0])
        } catch (e) {
            console.log("bad", String(e).split("dyn.qml:")[1])
        }
    }
}
