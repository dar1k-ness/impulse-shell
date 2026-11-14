import QtQuick
import QtQuick.Layouts
Item {
    id: root; width: 400; height: 400
    Loader { objectName: "ld1"; sourceComponent: Rectangle { objectName: "ld1item"; implicitWidth: 40; implicitHeight: 30 } }
    Loader { objectName: "ld2"; y: 40; width: 100; height: 50; sourceComponent: Rectangle { objectName: "ld2item"; implicitWidth: 40; implicitHeight: 30 } }
    Loader { objectName: "ld3"; y: 100; width: 100; sourceComponent: Rectangle { objectName: "ld3item"; width: 40; height: 30 } }
    Loader { objectName: "ld4"; y: 140; sourceComponent: Rectangle { objectName: "ld4item"; width: 40; height: 30 } }
    Loader { objectName: "ld5"; y: 180; active: false; width: 20; sourceComponent: Rectangle { implicitWidth: 40; implicitHeight: 30 } }
    Loader { objectName: "ld6"; y: 220; anchors.right: parent.right; sourceComponent: RowLayout { objectName: "ld6item"; Rectangle { implicitWidth: 13; implicitHeight: 17 } Rectangle { implicitWidth: 13; implicitHeight: 7 } } }
    Loader { objectName: "ld7"; y: 260; anchors.left: parent.left; anchors.right: parent.right; sourceComponent: Item { objectName: "ld7item"; implicitHeight: 12; Rectangle { objectName: "ld7fill"; anchors.fill: parent; anchors.margins: 1 } } }
    Loader { id: l8; objectName: "ld8"; y: 300; sourceComponent: Rectangle { objectName: "ld8item"; implicitWidth: 10; implicitHeight: 10 } }
    Timer { interval: 50; running: true; onTriggered: { l8.item.implicitWidth = 77; l8.height = 44 } }
    Item { objectName: "wrap"; y: 350; implicitWidth: l9.implicitWidth; implicitHeight: l9.implicitHeight; width: implicitWidth; height: implicitHeight
        Loader { id: l9; objectName: "ld9"; anchors.centerIn: parent; sourceComponent: Rectangle { implicitWidth: 31; implicitHeight: 21 } } }
}
