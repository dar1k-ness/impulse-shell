import QtQuick
Item {
    id: root; width: 400; height: 400
    Loader { id: a; objectName: "la"; width: 50; height: 50; sourceComponent: Rectangle { objectName: "laItem"; implicitWidth: 10; implicitHeight: 10 } }
    Loader { id: b; objectName: "lb"; y: 60; width: 50; height: 50; sourceComponent: Rectangle { objectName: "lbItem"; implicitWidth: 10; implicitHeight: 10 } }
    Loader { id: c; objectName: "lc"; y: 120; width: 50; height: 50; sourceComponent: Rectangle { objectName: "lcItem"; implicitWidth: 10; implicitHeight: 10 } }
    Loader { id: d; objectName: "ld"; y: 180; width: 50; height: 50; sourceComponent: Rectangle { objectName: "ldItem"; width: 22; height: 12 } }
    Loader { id: e; objectName: "le"; y: 240; sourceComponent: Rectangle { objectName: "leItem"; implicitWidth: 10; implicitHeight: 10 } }
    Loader { id: f; objectName: "lf"; y: 300; width: 50; height: 50; sourceComponent: Rectangle { objectName: "lfItem"; implicitWidth: 10; implicitHeight: 10 } }
    Timer { interval: 50; running: true; onTriggered: {
        a.width = undefined
        b.height = undefined
        c.width = undefined; c.height = undefined
        d.width = undefined
        e.width = 33
        f.width = undefined
    } }
    Timer { interval: 150; running: true; onTriggered: {
        e.width = undefined
        f.item.width = undefined
    } }
}
