import QtQuick as QQ

QQ.Item {
    id: root
    property real a: 0
    property real b: 0
    property real c: 0
    property real d: 0
    property QQ.color e: "red"
    property string log: ""
    QQ.NumberAnimation {
        id: na
        target: root
        property: "a"
        from: 0
        to: 100
        duration: 40
        onStarted: console.log("na started", running, paused)
        onFinished: {
            console.log("na finished", root.a, running)
            seq.start()
        }
    }
    QQ.NumberAnimation on b {
        id: onB
        to: 7
        duration: 20
    }
    QQ.Behavior on c {
        id: beh
        QQ.NumberAnimation {
            id: behAnim
            duration: 20
        }
    }
    QQ.SequentialAnimation {
        id: seq
        QQ.ScriptAction {
            script: root.log += "s1 "
        }
        QQ.PauseAnimation {
            duration: 10
        }
        QQ.PropertyAction {
            target: root
            property: "d"
            value: 5
        }
        QQ.ParallelAnimation {
            QQ.NumberAnimation {
                target: root
                property: "d"
                to: 9
                duration: 10
            }
            QQ.ColorAnimation {
                target: root
                property: "e"
                to: "blue"
                duration: 10
            }
        }
        QQ.ScriptAction {
            script: {
                root.log += "s2"
                console.log("seq", root.log, root.d, root.e, root.b, root.c, onB.running, behAnim.running)
                Qt.quit()
            }
        }
    }
    QQ.Component.onCompleted: {
        console.log("methods", typeof na.pause, typeof na.resume, typeof na.complete, typeof na.restart, typeof seq.start)
        console.log("defaults", na.duration, na.loops, na.running, na.paused, na.alwaysRunToEnd, na.easing.type, onB.running, beh.enabled, behAnim.duration)
        na.start()
        console.log("running", na.running, na.paused)
        na.pause()
        console.log("paused", na.running, na.paused)
        na.resume()
        console.log("resumed", na.running, na.paused)
        root.c = 50
        console.log("behavior", root.c !== 50, behAnim.running)
        console.log("names", String(na).split("(")[0], String(seq).split("(")[0], String(beh).split("(")[0])
    }
}
