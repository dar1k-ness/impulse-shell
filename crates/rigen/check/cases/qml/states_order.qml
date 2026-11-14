// QQuickStateGroup: stateChanged is emitted before the new state's values are applied; an
// assigned `state` stands over a State whose `when` holds until a `when` changes; a state
// that is left gives back the values its targets had before it; StateChangeScript runs when
// its state is entered, not when the document loads.
// qt: start::10 | state:big:10,1,1 100,0.5,2 | 7,8 | state::7,0.5,8 10,1,4 | state:other:10,1,4 script:55 55 | state:nosuch:55,1,4 nosuch:10 | state::10,1,4 10 | state:big:10,1,4 state:other:100,0.5,8 script:55 other:55 | other:55 | state:big:55,1,4 big:100
import QtQuick
Item {
    id: root
    property string result: ""
    property int v: 1
    property bool flag: false
    function log(s) {
        result += s + " "
    }
    Item {
        id: r
        width: 10
        opacity: 1
        property int z2: root.v
    }
    states: [
        State {
            name: "big"
            when: root.flag
            PropertyChanges {
                target: r
                width: 100
            }
            PropertyChanges {
                r.opacity: 0.5
                r.z2: root.v * 2
            }
        },
        State {
            name: "other"
            PropertyChanges {
                target: r
                width: 55
            }
            StateChangeScript {
                script: root.log("script:" + r.width)
            }
        }
    ]
    onStateChanged: log("state:" + state + ":" + [r.width, r.opacity, r.z2])
    Component.onCompleted: {
        log("start:" + state + ":" + r.width + " |")
        flag = true
        log([r.width, r.opacity, r.z2] + " |")
        v = 4
        r.width = 7
        log([r.width, r.z2] + " |")
        flag = false
        log([r.width, r.opacity, r.z2] + " |")
        state = "other"
        log(r.width + " |")
        state = "nosuch"
        log(state + ":" + r.width + " |")
        state = ""
        log(r.width + " |")
        flag = true
        state = "other"
        log(state + ":" + r.width + " |")
        flag = false
        log(state + ":" + r.width + " |")
        flag = true
        result += state + ":" + r.width
    }
}
