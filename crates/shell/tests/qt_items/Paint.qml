import QtQuick

// Scene of ../qt_items.rs (pixels) and tst_qt_items.qml (the same pixels in Qt 6.12).
Rectangle {
    width: 240; height: 150
    color: "black"

    // Parent opacity: 200,100,40 at 0.5 is 100,50,20; with the child's own 0.5 it is 50,25,10.
    Item {
        x: 10; y: 10; width: 30; height: 20
        opacity: 0.5
        Rectangle { width: 30; height: 10; color: "#c86428" }
        Rectangle { y: 10; width: 30; height: 10; color: "#c86428"; opacity: 0.5 }
    }

    // clip: the 60x60 child shows only inside the 20x20 item.
    Item {
        x: 70; y: 20; width: 20; height: 20
        clip: true
        Rectangle { x: -10; y: -10; width: 60; height: 60; color: "#00ff00" }
    }
    Item {
        x: 70; y: 80; width: 20; height: 20
        Rectangle { x: -10; y: -10; width: 60; height: 60; color: "#00ff00" }
    }

    // Corner radii: 20 top left, 0 top right, 10 bottom left; bottom right keeps `radius`.
    Rectangle {
        x: 140; y: 10; width: 40; height: 40
        color: "white"
        radius: 6
        topLeftRadius: 20
        topRightRadius: 0
        bottomLeftRadius: 10
    }

    // elide: each text is wider than its 70 px; only the last one paints past them.
    Column {
        x: 140; y: 60
        Repeater {
            model: [Text.ElideRight, Text.ElideLeft, Text.ElideMiddle, Text.ElideNone]
            Text {
                required property int modelData
                width: 70; height: 20
                color: "white"
                font.family: "DejaVu Sans"
                font.pixelSize: 14
                text: "first words and the last"
                elide: modelData
            }
        }
    }
}
