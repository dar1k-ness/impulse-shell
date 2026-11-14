import QtQuick
import Rustshell
FloatingWindow { visible: true; implicitWidth: 300; implicitHeight: 200
Item {
    id: root
    width: 300; height: 200
    property var messages: [{ t: "one" }, { t: "two" }]
    property bool busy: false
    property string tail: ""
    ListView {
        id: lv
        anchors.fill: parent
        spacing: 10
        topMargin: 30
        add: null
        onCountChanged: if (atYEnd || root.busy) Qt.callLater(positionViewAtEnd)
        Behavior on contentY { NumberAnimation { duration: 400 } }
        model: ScriptModel { values: root.messages }
        delegate: Rectangle {
            id: row
            required property var modelData
            required property int index
            width: 300
            height: 50 + (index === lv.count - 1 ? root.tail.length * 20 : 0)
        }
    }
    function say(tag) { console.log(tag, lv.count, lv.contentY, lv.contentHeight, lv.atYBeginning, lv.atYEnd) }
    function push(t) { const m = root.messages.slice(); m.push({ t: t }); root.messages = m }
    property int step: 0
    property var steps: [
        () => say("start"),
        () => { push("three"); push("four"); say("prompt") },
        () => say("prompt later"),
        () => { root.busy = true; push("answer"); say("answer") },
        () => say("answer later"),
        () => { root.tail = "a"; say("part 1") },
        () => say("part 1 later"),
        () => { root.tail = "ab"; say("part 2") },
        () => say("part 2 later"),
        () => { root.tail = "abc"; say("part 3") },
        () => say("part 3 later"),
        () => { root.tail = ""; push("next"); say("next") },
        () => say("next later"),
        () => { root.tail = "abcd"; say("next part") },
        () => say("next part later"),
        () => { root.busy = false; lv.contentY = 40; say("scrolled") },
        () => say("scrolled later"),
        () => { root.tail = ""; push("away"); say("away") },
        () => say("away later"),
        () => { lv.positionViewAtEnd(); say("button") },
        () => say("button later")
    ]
    Timer { interval: 600; repeat: true; running: true; onTriggered: {
        if (root.step >= root.steps.length) { console.log("done"); running = false; Qt.quit(); return }
        root.steps[root.step++]()
    } }
}
}
