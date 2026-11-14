import QtQuick

// Scene of ../item_transform.rs (pixels) and tst_item_transform.qml (the same pixels in Qt 6.12).
Rectangle {
    width: 400; height: 300
    color: "black"
    property alias shift: shift

    // Translate: drawn at 40..60 x 15..35. `shift.x` is changed by the test.
    Rectangle {
        x: 10; y: 10; width: 20; height: 20; color: "#ff0000"
        transform: Translate { id: shift; x: 30; y: 5 }
    }
    // Scale about (10, 10): 90..130 x 15..25.
    Rectangle {
        x: 100; y: 10; width: 20; height: 20; color: "#00ff00"
        transform: Scale { origin.x: 10; origin.y: 10; xScale: 2; yScale: 0.5 }
    }
    // Rotation by 90 about the top left corner: 190..200 x 10..50.
    Rectangle {
        x: 200; y: 10; width: 40; height: 10; color: "#0000ff"
        transform: Rotation { angle: 90 }
    }
    // The item's own scale goes first, then the list: 310..350 x 10..50.
    Rectangle {
        x: 300; y: 10; width: 20; height: 20; color: "#ff00ff"
        scale: 2; transformOrigin: Item.TopLeft
        transform: Translate { x: 10 }
    }
    // A list goes from its first entry to its last: scaled then moved is 50..90,
    // moved then scaled is 80..120.
    Rectangle {
        x: 20; y: 80; width: 20; height: 10; color: "#ffff00"
        transform: [ Scale { xScale: 2 }, Translate { x: 30 } ]
    }
    Rectangle {
        x: 20; y: 100; width: 20; height: 10; color: "#ffff00"
        transform: [ Translate { x: 30 }, Scale { xScale: 2 } ]
    }
    // Matrix4x4: x doubled and moved by 10, y moved by 20: 210..250 x 100..120.
    Rectangle {
        x: 200; y: 80; width: 20; height: 20; color: "#00ffff"
        transform: Matrix4x4 { matrix: Qt.matrix4x4(2, 0, 0, 10,  0, 1, 0, 20,  0, 0, 1, 0,  0, 0, 0, 1) }
    }
    // A child's Translate inside a parent's Scale: 320..340 x 80..90.
    Item {
        x: 300; y: 80; width: 10; height: 10
        transform: Scale { xScale: 2 }
        Rectangle {
            width: 10; height: 10; color: "#808080"
            transform: Translate { x: 10 }
        }
    }
    // clip moves with the item: the 60x60 child shows in 70..110 x 150..190.
    Item {
        x: 20; y: 150; width: 40; height: 40
        clip: true
        transform: Translate { x: 50 }
        Rectangle { x: -10; y: -10; width: 60; height: 60; color: "#ffffff" }
    }
    // clip turns with the item: the child shows in 180..200 x 150..190.
    Item {
        x: 200; y: 150; width: 40; height: 20
        clip: true
        transform: Rotation { angle: 90 }
        Rectangle { x: -10; y: -10; width: 60; height: 40; color: "#ffffff" }
    }
    // A parent's clip cuts a moved child: the child is at 330..350, shown up to 340.
    Item {
        x: 300; y: 150; width: 40; height: 40
        clip: true
        Rectangle {
            width: 20; height: 20; color: "#ffffff"
            transform: Translate { x: 30 }
        }
    }
    // Rotation about the y axis: the 100 wide rectangle is drawn narrower, with perspective.
    Rectangle {
        x: 20; y: 220; width: 100; height: 50; color: "#ff8000"
        transform: Rotation { origin.x: 50; origin.y: 25; axis { x: 0; y: 1; z: 0 } angle: 60 }
    }
}
