import QtQuick
import QtQuick.Layouts
Item {
    id: root; width: 400; height: 400
    Loader { id: l8; objectName: "ld8"; sourceComponent: Rectangle { objectName: "ld8item"; implicitWidth: 10; implicitHeight: 10 } }
    Loader { id: l9; objectName: "ld9"; y: 20; sourceComponent: Rectangle { objectName: "ld9item"; implicitWidth: 10; implicitHeight: 10 } }
    Rectangle { id: r1; objectName: "r1"; y: 40; implicitWidth: 10; implicitHeight: 10 }
    Rectangle { id: r2; objectName: "r2"; y: 60; implicitWidth: 10; implicitHeight: 10; width: 30 }
    Rectangle { id: r3; objectName: "r3"; y: 80; implicitWidth: 10; implicitHeight: 10; width: 30 }
    RowLayout { id: rl; objectName: "rl"; y: 100
        Rectangle { objectName: "rla"; implicitWidth: 10; implicitHeight: 10 }
        Rectangle { id: rlb; objectName: "rlb"; implicitWidth: 10; implicitHeight: 10 }
    }
    Text { id: tx; objectName: "tx"; y: 130; text: "hello" }
    Column { id: col; objectName: "col"; y: 160; Rectangle { width: 10; height: 10 } }
    Loader { id: l11; objectName: "ld11"; y: 260; sourceComponent: Rectangle { objectName: "ld11item"; width: 22; height: 12 } }
    Rectangle { id: r4; objectName: "r4"; y: 300; implicitWidth: 10; implicitHeight: 10; height: 30 }
    Rectangle { id: r5; objectName: "r5"; y: 340; implicitWidth: 10; implicitHeight: 10 }
    Timer { interval: 50; running: true; onTriggered: {
        l8.item.implicitWidth = 77; l8.height = 44
        l9.width = 33
        r1.height = 44
        r2.width = undefined
        r3.implicitWidth = 55
        rl.width = 100
        tx.width = 5
        col.height = 99
        l11.item.width = 60
        r4.height = undefined
        r5.width = 25; r5.implicitWidth = 70
    } }
}
