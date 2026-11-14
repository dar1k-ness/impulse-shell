import QtQuick as QQ

QQ.Item {
    id: root
    property bool on: false
    property real v: 1
    property string seen: ""
    states: [
        QQ.State {
            name: "lit"
            when: root.on
            QQ.PropertyChanges {
                target: root
                v: 10
            }
            QQ.StateChangeScript {
                script: root.seen += "script "
            }
        }
    ]
    transitions: [
        QQ.Transition {
            to: "lit"
            QQ.SequentialAnimation {
                QQ.NumberAnimation {
                    target: root
                    property: "v"
                    duration: 20
                }
                QQ.ScriptAction {
                    script: {
                        console.log("transition end", root.v, root.state, root.seen)
                        Qt.quit()
                    }
                }
            }
        }
    ]
    QQ.Component.onCompleted: {
        console.log("before", state === "", v, states.length, transitions.length, String(states[0]).split("(")[0])
        on = true
        console.log("after", state, v < 10, transitions[0].running)
    }
}
