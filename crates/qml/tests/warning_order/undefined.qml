// A binding that gives `undefined` to a property that takes none: Qt says `Unable to assign
// [undefined] to <type>`, and holds that back as it holds the other errors of bindings back,
// until the creation the binding ran in is over. The ones that gave a value meanwhile are not
// said at all. The bindings are those of the original's bar: `occupied[index]` of a list filled in
// `Component.onCompleted`, and a child of a Row a Repeater has yet to fill.
import QtQuick

Item {
    id: root
    property list<bool> occupied: []
    property list<bool> never: []
    property int current: 0

    Row {
        id: row
        Repeater {
            model: 3
            Item {
                required property int index
                property bool here: root.occupied[index] && index >= 0
                property bool next: index < 2 && root.occupied[index + 1] && index >= 0
                implicitWidth: 10 + index
            }
        }
    }
    Item {
        property Item target: row.children[root.current]
        onTargetChanged: console.log("target", target ? target.implicitWidth : target)
    }
    // These stand when the creation is over: said then, the one that failed last first.
    Item {
        property bool stays: root.never[0]
    }
    Item {
        property int staysToo: root.never[1]
    }

    Component {
        id: recovers
        Item {
            property bool late: root.never[2]
            Component.onCompleted: {
                console.log("made: completed")
                root.never = [true, true, true]
            }
        }
    }
    Component {
        id: fails
        Item {
            property real late: root.never[3]
            Component.onCompleted: console.log("made: completed")
        }
    }

    Component.onCompleted: {
        console.log("completed, here", row.children[0].here, row.children[1].here)
        root.occupied = [true, false, true]
        console.log("filled, here", row.children[0].here, row.children[1].here)
    }
    Timer {
        interval: 1
        running: true
        onTriggered: {
            console.log("timer 1 an object whose binding recovers")
            recovers.createObject(root)
            console.log("timer 2 an object whose binding does not")
            fails.createObject(root)
            console.log("timer 3 the list is emptied")
            root.occupied = []
            console.log("timer 4 the end, here", row.children[0].here, row.children[1].here)
            Qt.quit()
        }
    }
}
