// Where a delegate running its `remove` transition, the delegates around it and the view's
// attached ScrollBar are on each frame. Frame 2 removes the second delegate of each list:
// `anchored` has delegates anchored to the view's sides (the x the transition writes does not
// hold), `sized` has delegates with a width, `tall` is taller than its content (the bar's
// size is 1), `row` is horizontal. Every delegate is in view from the start, so none is made
// later. After each frame the log gets the bar's x, y, width, height, size and position (not
// for `row`) and every delegate's n, x, y, width, height and opacity.
import QtQuick
import QtQuick.Controls
Item {
    id: scene
    width: 500; height: 210
    property int frames: 28
    property string log: ""
    Component {
        id: numbers
        ListModel {
            ListElement { n: 1 }
            ListElement { n: 2 }
            ListElement { n: 3 }
            ListElement { n: 4 }
            ListElement { n: 5 }
            ListElement { n: 16 }
        }
    }
    Component {
        id: leave
        Transition {
            NumberAnimation { property: "x"; to: 140; duration: 200 }
            NumberAnimation { property: "opacity"; to: 0; duration: 200 }
        }
    }
    Component {
        id: close
        Transition {
            NumberAnimation { properties: "x,y"; duration: 120 }
            NumberAnimation { properties: "opacity,scale"; to: 1; duration: 120 }
        }
    }
    ListView {
        id: anchored
        property var live: []
        width: 120; height: 145
        spacing: 5
        model: numbers.createObject(anchored)
        remove: leave.createObject(anchored)
        removeDisplaced: close.createObject(anchored)
        ScrollBar.vertical: ScrollBar { contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
        delegate: Rectangle {
            required property int n
            anchors.left: parent?.left
            anchors.right: parent?.right
            height: 20 + n
            Component.onCompleted: anchored.live.push(this)
            Component.onDestruction: anchored.live.splice(anchored.live.indexOf(this), 1)
        }
    }
    ListView {
        id: sized
        property var live: []
        x: 130; width: 120; height: 145
        spacing: 5
        model: numbers.createObject(sized)
        remove: leave.createObject(sized)
        removeDisplaced: close.createObject(sized)
        ScrollBar.vertical: ScrollBar { contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
        delegate: Rectangle {
            required property int n
            width: 100; height: 20 + n
            Component.onCompleted: sized.live.push(this)
            Component.onDestruction: sized.live.splice(sized.live.indexOf(this), 1)
        }
    }
    ListView {
        id: tall
        property var live: []
        x: 260; width: 120; height: 200
        spacing: 5
        model: numbers.createObject(tall)
        remove: leave.createObject(tall)
        removeDisplaced: close.createObject(tall)
        ScrollBar.vertical: ScrollBar { contentItem: Rectangle { implicitWidth: 6; implicitHeight: 6 } }
        delegate: Rectangle {
            required property int n
            width: 100; height: 20 + n
            Component.onCompleted: tall.live.push(this)
            Component.onDestruction: tall.live.splice(tall.live.indexOf(this), 1)
        }
    }
    ListView {
        id: row
        property var live: []
        x: 390; width: 145; height: 60
        orientation: ListView.Horizontal
        spacing: 5
        model: numbers.createObject(row)
        remove: leave.createObject(row)
        removeDisplaced: close.createObject(row)
        delegate: Rectangle {
            required property int n
            width: 20 + n; height: 40
            Component.onCompleted: row.live.push(this)
            Component.onDestruction: row.live.splice(row.live.indexOf(this), 1)
        }
    }
    function sample(frame) {
        if (frame === 2)
            for (const l of [anchored, sized, tall, row])
                l.model.remove(1)
    }
    function shown(frame) {
        const lists = { anchored: anchored, sized: sized, tall: tall, row: row }
        for (const k in lists) {
            const l = lists[k]
            const r = v => +v.toFixed(2)
            let s = frame + " " + k
            if (k !== "row") {
                const bar = l.ScrollBar.vertical
                s += " bar " + [bar.x, bar.y, bar.width, bar.height, bar.size, bar.position].map(r).join(" ")
            }
            const ds = l.live.slice().sort((a, b) => a.n - b.n)
            for (const d of ds)
                s += " | " + [d.n, d.x, d.y, d.width, d.height, d.opacity].map(r).join(" ")
            log += s + "\n"
        }
    }
}
