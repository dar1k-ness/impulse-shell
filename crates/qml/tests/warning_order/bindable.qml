import QtQuick

Item {
    id: root
    property list<real> xs: []
    property list<string> names: []
    property list<real> never: []

    Item { id: a; x: root.xs[0] }
    Item { id: b; objectName: root.names[0] }
    Item { id: c; x: root.never[0] }
    Item { id: d; property real w: root.xs[0] }

    Component.onCompleted: {
        console.log("completed", a.x, b.objectName, c.x, d.w)
        root.xs = [3]
        root.names = ["n"]
        console.log("filled", a.x, b.objectName, c.x, d.w)
    }

    Timer {
        interval: 1
        running: true
        onTriggered: {
            console.log("timer the list is emptied")
            root.xs = []
            console.log("timer the end", a.x, d.w)
            Qt.quit()
        }
    }
}
