import QtQuick

// Scene of ../item_transform.rs (clicks) and tst_item_transform.qml (the same clicks in Qt 6.12).
// Each area logs its name and where in it the click landed.
Item {
    id: root
    width: 400; height: 200
    property string log: ""
    function hit(name, mouse) {
        log += name + " " + Math.round(mouse.x * 10) / 10 + "," + Math.round(mouse.y * 10) / 10 + " "
    }

    MouseArea { anchors.fill: parent; onClicked: root.log += "back " }

    // Translate: the area is at 40..60 x 10..30.
    MouseArea {
        x: 10; y: 10; width: 20; height: 20
        transform: Translate { x: 30 }
        onClicked: mouse => root.hit("moved", mouse)
    }
    // Scale about (10, 10): 90..130 x 15..25.
    MouseArea {
        x: 100; y: 10; width: 20; height: 20
        transform: Scale { origin.x: 10; origin.y: 10; xScale: 2; yScale: 0.5 }
        onClicked: mouse => root.hit("scaled", mouse)
    }
    // Rotation by 90 about the top left corner: 190..200 x 10..50.
    MouseArea {
        x: 200; y: 10; width: 40; height: 10
        transform: Rotation { angle: 90 }
        onClicked: mouse => root.hit("turned", mouse)
    }
    // Scaled then moved: 230..270 x 80..90.
    MouseArea {
        x: 200; y: 80; width: 20; height: 10
        transform: [ Scale { xScale: 2 }, Translate { x: 30 } ]
        onClicked: mouse => root.hit("list", mouse)
    }
    // clip moves with the item: the 100 wide area takes clicks in 70..110 x 60..100.
    Item {
        x: 20; y: 60; width: 40; height: 40
        clip: true
        transform: Translate { x: 50 }
        MouseArea { width: 100; height: 40; onClicked: mouse => root.hit("clipped", mouse) }
    }
    // Rotation about the y axis: narrower than the 100 it is wide.
    MouseArea {
        x: 20; y: 120; width: 100; height: 50
        transform: Rotation { origin.x: 50; origin.y: 25; axis { x: 0; y: 1; z: 0 } angle: 60 }
        onClicked: mouse => root.hit("tilted", mouse)
    }
    // Scaled to nothing: takes no click.
    MouseArea {
        x: 300; y: 120; width: 50; height: 50
        transform: Scale { xScale: 0 }
        onClicked: mouse => root.hit("flat", mouse)
    }
}
