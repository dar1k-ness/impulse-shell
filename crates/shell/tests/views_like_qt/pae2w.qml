import QtQuick
import Rustshell
FloatingWindow { visible: true; implicitWidth: 300; implicitHeight: 200
Item {
    id: root
    width: 300; height: 200
    property var names: ["a", "b"]
    property real extra: 0
    ListView {
        id: lv
        anchors.fill: parent
        spacing: 10
        topMargin: 30
        model: ScriptModel { values: root.names }
        onCountChanged: if (atYEnd) Qt.callLater(positionViewAtEnd)
        Behavior on contentY { NumberAnimation { duration: 400 } }
        delegate: Rectangle { required property var modelData; width: 300; height: modelData === "f" ? 50 + root.extra : 50 }
    }
    function say(tag) { console.log(tag, lv.count, lv.contentY, lv.contentHeight, lv.originY, lv.atYBeginning, lv.atYEnd) }
    property int step: 0
    property var all: ["a", "b", "c", "d", "e", "f"]
    property var steps: [
        () => say("start"),
        () => { root.names = root.all.slice(0, 3); say("3") },
        () => say("3 later"),
        () => { root.names = root.all.slice(0, 4); say("4") },
        () => say("4 later"),
        () => { root.names = root.all.slice(0, 5); say("5") },
        () => say("5 later"),
        () => { root.names = root.all.slice(0, 6); say("6") },
        () => say("6 later"),
        () => { root.extra = 40; say("grown") },
        () => say("grown later"),
        () => { lv.contentY = 100; say("to 100") },
        () => say("at 100"),
        () => { root.extra = 80; say("grown more") },
        () => say("grown more later"),
        () => { lv.positionViewAtEnd(); say("end") },
        () => say("end later"),
        () => { lv.positionViewAtBeginning(); say("beginning") },
        () => say("beginning later")
    ]
    Timer { interval: 600; repeat: true; running: true; onTriggered: {
        if (root.step >= root.steps.length) { console.log("done"); running = false; Qt.quit(); return }
        root.steps[root.step++]()
    } }
}
}
