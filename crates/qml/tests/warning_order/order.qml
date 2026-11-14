// Each warning of the engine is raised between two console lines: Qt prints it between them.
import QtQuick

Item {
    id: root
    property int n: 0
    property var o: ({ x: 1 })
    property int viaO: o.x
    property int viaO2: o.x + 1
    property int never: missing.x
    signal poked()

    NumberAnimation { id: idle; target: root; property: "n"; to: 5; duration: 100 }
    SequentialAnimation {
        id: group
        NumberAnimation { id: inner; target: root; property: "n"; to: 7; duration: 100 }
    }

    onNChanged: {
        console.log("handler", n)
        if (n === 1)
            throw new Error("thrown by onNChanged")
    }
    Connections {
        target: root
        function onPoked() {
            console.log("connection")
            throw new Error("thrown by onPoked")
        }
    }

    // Qt holds the errors of bindings back until the creation they are raised in is over, and
    // says then the ones that still stand, so the same steps run once inside the creation and
    // once from a timer.
    function steps(from) {
        console.log(from, "1 pause an idle animation")
        idle.pause()
        console.log(from, "2 resume it")
        idle.resume()
        console.log(from, "3 start an animation of a group")
        inner.running = true
        console.log(from, "4 a handler throws")
        n = 1
        console.log(from, "5 a function of Connections throws")
        poked()
        console.log(from, "6 a binding fails")
        o = null
        console.log(from, "7 the end")
        n = 0
    }
    Component.onCompleted: steps("creation")
    Item {
        property int recovers: root.o.x
        Component.onCompleted: {
            console.log("child completed")
            root.o = { x: 2 }
            root.o = null
        }
    }
    Timer {
        interval: 1
        running: true
        onTriggered: {
            console.log("timer")
            root.o = { x: 3 }
            root.steps("timer")
            Qt.quit()
        }
    }
}
