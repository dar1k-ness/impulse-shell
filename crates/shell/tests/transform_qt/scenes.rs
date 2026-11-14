// The scenes of ../transform_pixels_qt.rs, each as `scene!(name, body)`, or as
// `scene!(name, cpu, gpu, body)` with the pixels known not to be Qt's on the CPU path and on
// the GPU path at each scale.

// Translate by a fraction of a pixel, by a binding on the item's own size, and of a rounded
// (antialiased) rectangle.
scene!(
    translate,
    r##"
Rectangle { x: 10.1; y: 10.1; width: 30; height: 20; color: "#ff0000"
    transform: Translate { x: 20.3; y: 7.6 } }
Rectangle { id: b; x: 70.1; y: 50.1; width: 20; height: 24; color: "#00ff00"
    transform: Translate { x: b.width / 2; y: -b.height / 4 } }
Rectangle { x: 110.1; y: 10.1; width: 30; height: 30; radius: 15; color: "#3366ff"
    transform: Translate { x: -5.4; y: 40.2 } }
"##
);

// Scale about an origin, by different factors along x and y, and mirrored.
scene!(
    scale,
    719,
    [0, 0, 0, 1],
    r##"
Rectangle { x: 20.1; y: 10.1; width: 20; height: 20; color: "#ff0000"
    transform: Scale { origin.x: 10; origin.y: 10; xScale: 1.7; yScale: 0.6 } }
Rectangle { x: 70.1; y: 10.1; width: 30; height: 20; color: "#00ff00"
    Rectangle { x: 2; y: 2; width: 8; height: 8; color: "#ffffff" }
    transform: Scale { origin.x: 15; xScale: -1; yScale: 2.5 } }
Rectangle { x: 20.1; y: 60.1; width: 24; height: 24; radius: 6; color: "#3366ff"
    border.width: 2; border.color: "#ffee00"
    transform: Scale { xScale: 2.2; yScale: 1.3 } }
"##
);

// Rotation about z: a quarter turn, and angles that leave the pixel grid.
scene!(
    rotation_z,
    r##"
Rectangle { x: 40.1; y: 10.1; width: 40; height: 12; color: "#ff0000"
    transform: Rotation { angle: 90 } }
Rectangle { x: 60.1; y: 30.1; width: 40; height: 24; color: "#00ff00"
    Rectangle { x: 4; y: 4; width: 10; height: 6; color: "#ffffff" }
    transform: Rotation { origin.x: 20; origin.y: 12; angle: 30 } }
Rectangle { x: 110.1; y: 60.1; width: 30; height: 20; color: "#3366ff"
    transform: Rotation { origin.x: 30; origin.y: 20; angle: -72 } }
"##
);

// Rotation about the x axis, the y axis and a slanted axis: drawn with perspective
// (QMatrix4x4::projectedRotate).
scene!(
    rotation_axis,
    411,
    [411, 588, 628, 684],
    r##"
Rectangle { x: 10.1; y: 10.1; width: 40; height: 36; color: "#ff0000"
    Rectangle { x: 4; y: 4; width: 10; height: 6; color: "#ffffff" }
    transform: Rotation { origin.x: 20; origin.y: 18; axis { x: 1; y: 0; z: 0 } angle: 50 } }
Rectangle { x: 60.1; y: 10.1; width: 40; height: 36; color: "#00ff00"
    Rectangle { x: 4; y: 4; width: 10; height: 6; color: "#ffffff" }
    transform: Rotation { origin.x: 20; origin.y: 18; axis { x: 0; y: 1; z: 0 } angle: -40 } }
Rectangle { x: 60.1; y: 55.1; width: 40; height: 36; color: "#3366ff"
    Rectangle { x: 4; y: 4; width: 10; height: 6; color: "#ffffff" }
    transform: Rotation { origin.x: 20; origin.y: 18; axis { x: 1; y: 1; z: 0 } angle: 35 } }
"##
);

// Matrix4x4: a shear with a move, and a matrix with a perspective row.
scene!(
    matrix,
    114,
    [114, 154, 171, 211],
    r##"
Rectangle { x: 10.1; y: 10.1; width: 30; height: 20; color: "#ff0000"
    Rectangle { x: 3; y: 3; width: 8; height: 6; color: "#ffffff" }
    transform: Matrix4x4 { matrix: Qt.matrix4x4(1.5, 0.4, 0, 12,  -0.3, 1.2, 0, 9,  0, 0, 1, 0,  0, 0, 0, 1) } }
Rectangle { x: 80.1; y: 40.1; width: 40; height: 30; color: "#00ff00"
    Rectangle { x: 3; y: 3; width: 8; height: 6; color: "#ffffff" }
    transform: Matrix4x4 { matrix: Qt.matrix4x4(1, 0, 0, 0,  0, 1, 0, 0,  0, 0, 1, 0,  0.004, 0.006, 0, 1) } }
"##
);

// Shear (Qt 6.9), by factors and by angles about an origin.
scene!(
    shear,
    r##"
Rectangle { x: 20.1; y: 10.1; width: 30; height: 20; color: "#ff0000"
    transform: Shear { xFactor: 0.5 } }
Rectangle { x: 80.1; y: 50.1; width: 30; height: 20; color: "#00ff00"
    transform: Shear { origin.x: 15; origin.y: 10; xAngle: 20; yAngle: -15 } }
"##
);

// A list of three and the same three the other way round.
scene!(
    list_order,
    r##"
Rectangle { x: 30.1; y: 10.1; width: 24; height: 12; color: "#ff0000"
    Rectangle { width: 6; height: 6; color: "#ffffff" }
    transform: [ Rotation { angle: 25 }, Scale { xScale: 2; yScale: 1.5 }, Translate { x: 14.3; y: 9.2 } ] }
Rectangle { x: 90.1; y: 50.1; width: 24; height: 12; color: "#00ff00"
    Rectangle { width: 6; height: 6; color: "#ffffff" }
    transform: [ Translate { x: 14.3; y: 9.2 }, Scale { xScale: 2; yScale: 1.5 }, Rotation { angle: 25 } ] }
"##
);

// The list together with the item's own `scale`, `rotation` and `transformOrigin`.
scene!(
    with_item_scale_rotation,
    r##"
Rectangle { x: 20.1; y: 20.1; width: 30; height: 20; color: "#ff0000"
    Rectangle { width: 6; height: 6; color: "#ffffff" }
    scale: 1.4; rotation: 20; transformOrigin: Item.BottomRight
    transform: [ Translate { x: 6.5; y: -3.5 }, Scale { origin.x: 30; xScale: 0.8 } ] }
Rectangle { x: 80.1; y: 15.1; width: 30; height: 20; color: "#00ff00"
    Rectangle { width: 6; height: 6; color: "#ffffff" }
    scale: 0.7; rotation: -90; transformOrigin: Item.Top
    transform: Rotation { origin.x: 15; origin.y: 10; angle: 90 } }
Rectangle { x: 100.1; y: 60.1; width: 30; height: 20; color: "#3366ff"
    Rectangle { width: 6; height: 6; color: "#ffffff" }
    scale: 1.5; transformOrigin: Item.Left
    transform: Translate { x: 10.5 } }
"##
);

// `clip: true` on the transformed item itself: the clip turns, grows and tilts with it.
scene!(
    clip_on_item,
    548,
    [236, 328, 350, 410],
    r##"
Item { x: 30.1; y: 10.1; width: 40; height: 24; clip: true
    transform: Rotation { origin.x: 20; origin.y: 12; angle: 30 }
    Rectangle { x: -10; y: -10; width: 70; height: 50; color: "#ff0000" }
    Rectangle { x: 30; y: 14; width: 30; height: 30; color: "#ffffff" } }
Item { x: 90.1; y: 10.1; width: 20; height: 20; clip: true
    transform: Scale { xScale: 2.3; yScale: 1.6 }
    Rectangle { x: -5; y: -5; width: 30; height: 30; radius: 15; color: "#00ff00" } }
Item { x: 20.1; y: 60.1; width: 40; height: 30; clip: true
    transform: Rotation { origin.x: 20; origin.y: 15; axis { x: 0; y: 1; z: 0 } angle: 45 }
    Rectangle { x: -10; y: -10; width: 70; height: 50; color: "#3366ff" } }
"##
);

// `clip: true` on a parent: it cuts children that their transforms move, turn or grow
// across its edge, also when the parent is transformed itself.
scene!(
    clip_on_parent,
    226,
    [99, 123, 144, 169],
    r##"
Item { x: 10.1; y: 10.1; width: 40; height: 30; clip: true
    Rectangle { x: 10; y: 5; width: 20; height: 20; color: "#ff0000"
        transform: Translate { x: 22.4; y: 12.7 } }
    Rectangle { x: 0; y: 0; width: 16; height: 8; color: "#ffffff"
        transform: Rotation { angle: 40 } } }
Item { x: 70.1; y: 10.1; width: 40; height: 30; clip: true
    Rectangle { x: 10; y: 10; width: 10; height: 10; color: "#00ff00"
        transform: Scale { origin.x: 5; origin.y: 5; xScale: 5; yScale: 1.5 } } }
Item { x: 40.1; y: 55.1; width: 40; height: 30; clip: true
    transform: Rotation { angle: -15 }
    Rectangle { x: 20; y: 10; width: 30; height: 30; color: "#3366ff"
        transform: [ Scale { xScale: 1.5 }, Translate { x: -8.5 } ] } }
"##
);

// Transforms inside transforms, with opacity on the way.
scene!(
    nested,
    r##"
Item { x: 60.1; y: 10.1; width: 60; height: 40
    transform: Rotation { angle: 20 }
    Rectangle { x: 5; y: 5; width: 40; height: 30; color: "#ff0000"; opacity: 0.6
        transform: Scale { origin.x: 20; origin.y: 15; xScale: 1.3; yScale: 0.8 }
        Rectangle { x: 10; y: 5; width: 12; height: 12; color: "#ffffff"
            rotation: 45
            transform: Translate { x: 8.5; y: 3.5 } } } }
"##
);
