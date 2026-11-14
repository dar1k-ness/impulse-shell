import QtQuick

// Scene of ../qt_items.rs (clicks) and tst_qt_items.qml (the same clicks in Qt 6.12).
Item {
    id: root
    width: 200; height: 100
    property string log: ""
    property alias gate: gate
    property alias inner: inner
    property alias plain: plain
    property alias under: under
    property alias conn: conn
    signal ping()

    MouseArea { anchors.fill: parent; onClicked: root.log += "back " }

    // The area is 100 wide inside a 40 wide item.
    Item {
        x: 10; y: 10; width: 40; height: 40
        clip: true
        MouseArea { width: 100; height: 40; onClicked: root.log += "clipped " }
    }
    Item {
        x: 10; y: 55; width: 40; height: 40
        MouseArea { width: 100; height: 40; onClicked: root.log += "unclipped " }
    }

    Item {
        id: gate
        x: 150; y: 10; width: 40; height: 40
        enabled: false
        MouseArea { id: inner; anchors.fill: parent; onClicked: root.log += "inner " }
        Item { id: plain }
    }

    // MouseArea.enabled is the area's own property: off, it takes no click itself and leaves
    // the items in it enabled.
    MouseArea {
        x: 150; y: 55; width: 40; height: 40
        enabled: false
        onClicked: root.log += "off-area "
        Item { id: under }
        MouseArea { width: 20; height: 40; onClicked: root.log += "under " }
    }

    Connections {
        id: conn
        target: root
        enabled: false
        function onPing() { root.log += "off " }
    }
    Connections {
        target: root
        function onPing() { root.log += "on " }
    }
}
