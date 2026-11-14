//! A plain Rectangle whose edges fall inside a device pixel, on a screen whose scale is not 1,
//! against Qt itself (support/qt_window.rs). Qt antialiases a Rectangle only when it has a
//! radius or `antialiasing` is set, and rounds an aligned border to whole device pixels.

#[path = "support/qt_window.rs"]
mod qt_window;

const DPRS: [f64; 4] = [1.25, 1.5, 1.75, 2.0];

/// No edge of these scenes is nearer than a tenth of a device pixel to a pixel's centre at any
/// of DPRS: there the GPU's rule decides alone, with no tie to break.
fn like_qt(body: &str) {
    qt_window::like_qt(body, (120, 64), &DPRS, 2, 0);
}

#[test]
fn plain_rectangle() {
    like_qt(
        "Rectangle { x: 8.1; y: 4.1; width: 22; height: 14; color: \"#3366ff\" }\n\
         Rectangle { x: 44.1; y: 4.1; width: 30.1; height: 20.1; color: \"#ffee00\"; opacity: 0.5 }\n\
         Rectangle { x: 84.2; y: 8.2; width: 24; height: 10; color: \"#80ee2211\" }\n\
         Item { x: 0.1; y: 30.1; Rectangle { x: 10.1; y: 2; width: 2; height: 24; color: \"white\" }\n\
             Rectangle { x: 20; y: 4.1; width: 60; height: 2; color: \"white\" } }",
    );
}

#[test]
fn plain_rectangle_with_border() {
    like_qt(
        "Rectangle { x: 8.1; y: 4.1; width: 30; height: 20; color: \"#3366ff\"; border.color: \"#ffee00\"; border.width: 2 }\n\
         Rectangle { x: 48.1; y: 4.1; width: 30; height: 20; color: \"#3366ff\"; border.color: \"#ffee00\"; border.width: 1 }\n\
         Rectangle { x: 88.1; y: 4.1; width: 24; height: 20; color: \"transparent\"; border.color: \"#ee2211\"; border.width: 3 }\n\
         Rectangle { x: 8.1; y: 34.1; width: 30; height: 20; color: \"#3366ff\"; border.color: \"#ffee00\"; border.width: 1.3 }\n\
         Rectangle { x: 48.1; y: 34.1; width: 30; height: 20; color: \"#3366ff\"; border.color: \"#ffee00\"; border.width: 0.45 }",
    );
}

#[test]
fn rounded_rectangle() {
    like_qt(
        "Rectangle { x: 8.1; y: 4.1; width: 30; height: 20; radius: 6; color: \"#3366ff\" }\n\
         Rectangle { x: 48.1; y: 4.1; width: 30; height: 20; topLeftRadius: 8; color: \"#ffee00\" }\n\
         Rectangle { x: 88.1; y: 4.1; width: 20; height: 20; radius: 10; color: \"#ee2211\"; opacity: 0.6 }",
    );
}

#[test]
fn rounded_rectangle_with_border() {
    like_qt(
        "Rectangle { x: 8.1; y: 4.1; width: 30; height: 20; radius: 6; color: \"#3366ff\"; border.color: \"#ffee00\"; border.width: 2 }\n\
         Rectangle { x: 48.1; y: 4.1; width: 30; height: 20; radius: 6; color: \"#3366ff\"; border.color: \"#ffee00\"; border.width: 1 }\n\
         Rectangle { x: 88.1; y: 4.1; width: 24; height: 20; bottomRightRadius: 7; color: \"transparent\"; border.color: \"#ee2211\"; border.width: 3 }\n\
         Rectangle { x: 8.1; y: 34.1; width: 30; height: 20; radius: 6; color: \"#3366ff\"; border.color: \"#ffee00\"; border.width: 1.3 }",
    );
}

/// `antialiasing` set wins over the value the radius implies, both ways.
#[test]
fn rectangle_antialiasing_set() {
    like_qt(
        "Rectangle { x: 8.1; y: 4.1; width: 30; height: 20; antialiasing: true; color: \"#3366ff\" }\n\
         Rectangle { x: 48.1; y: 4.1; width: 30; height: 20; antialiasing: true; color: \"#3366ff\"; border.color: \"#ffee00\"; border.width: 2 }\n\
         Rectangle { x: 88.1; y: 4.1; width: 24; height: 20; radius: 6; antialiasing: false; color: \"#ee2211\" }\n\
         Rectangle { x: 8.1; y: 34.1; width: 30; height: 20; radius: 6; antialiasing: false; color: \"#3366ff\"; border.color: \"#ffee00\"; border.width: 2 }",
    );
}

/// `border.pixelAligned: false` keeps the width QML gave.
#[test]
fn rectangle_border_not_pixel_aligned() {
    like_qt(
        "Rectangle { x: 8.1; y: 4.1; width: 30; height: 20; color: \"#3366ff\"; border.color: \"#ffee00\"; border.width: 1.3; border.pixelAligned: false }\n\
         Rectangle { x: 48.1; y: 4.1; width: 30; height: 20; radius: 6; color: \"#3366ff\"; border.color: \"#ffee00\"; border.width: 1.3; border.pixelAligned: false }\n\
         Rectangle { x: 88.1; y: 4.1; width: 24; height: 20; antialiasing: true; color: \"#3366ff\"; border.color: \"#ffee00\"; border.width: 0.45; border.pixelAligned: false }",
    );
}

/// A layer scales its source onto (size * devicePixelRatio) rounded once, so the edges of a
/// Rectangle in a source of 22.5 x 19.3 fall inside texels at every scale, and stay hard.
/// The layer is shown texel by texel: the effect is placed where no device pixel's centre
/// is on a texel's edge.
#[test]
fn plain_rectangle_in_a_layer() {
    like_qt(
        "Item { id: src; width: 22.5; height: 19.3; visible: false\n\
             Rectangle { x: 5.1; y: 3.8; width: 10.3; height: 9.7; color: \"#3366ff\" } }\n\
         Desaturate { x: 9.54; y: 8.62; width: 22.5; height: 19.3; source: src; desaturation: 0.5 }",
    );
}

/// A panel's outline: the window hears of the output's scale after its Rectangles were made,
/// at the screen's whole ratio, and Qt does not round their borders again to the new pixels.
#[test]
fn border_made_before_the_scale() {
    qt_window::like_qt_by(
        qt_window::Scale::OutputAfterLoad,
        "Rectangle { x: 8; y: 4; width: 40; height: 24; radius: 6; color: \"#3366ff\"; border.color: \"#ffee00\"; border.width: 1 }\n\
         Rectangle { x: 60; y: 4; width: 40; height: 24; color: \"#3366ff\"; border.color: \"#ffee00\"; border.width: 1 }\n\
         Rectangle { x: 8; y: 34; width: 40; height: 24; radius: 6; color: \"#3366ff\"; border.color: \"#ffee00\"; border.width: 2 }",
        (120, 64),
        &[1.25, 1.5, 2.0],
        2,
        0,
    );
}
