//! MultiEffect, GaussianBlur, DropShadow and the cached RectangularShadow on a screen whose
//! scale is not 1, and blur radii that are not whole numbers, against Qt itself:
//! tools/oracle/effects_oracle.py shows the same items in a QQuickView (OpenGL, under a headless
//! sway) at that devicePixelRatio and the window's device pixels are compared with ours.

#[path = "support/qt_window.rs"]
mod qt_window;

const SIZE: (usize, usize) = (120, 64);

fn like_qt(body: &str, dprs: &[f64], tol: u8, allow: usize) {
    qt_window::like_qt(body, SIZE, dprs, tol, allow)
}

/// A source with detail two scene pixels wide, hidden: the effects draw it. Its straight
/// edges are on even coordinates, whole device pixels at the scales tested, where Qt and we
/// rasterize alike; the tests keep the effects on such pixels too. An edge through the middle
/// of a device pixel is a tie the GPU breaks by the target's orientation.
const SRC: &str = r##"
    Item {
        id: src
        width: 40; height: 30
        visible: false
        Rectangle { anchors.fill: parent; radius: 9; color: "#3366ff" }
        Rectangle { x: 6; y: 4; width: 14; height: 2; color: "#ffee00" }
        Rectangle { x: 6; y: 8; width: 2; height: 14; color: "#ffffff" }
        Rectangle { x: 22; y: 10; width: 12; height: 12; radius: 6; color: "#ee2211" }
    }"##;

/// A source of rounded shapes only: antialiased, so it may sit on a fraction of a pixel.
const ROUND: &str = r##"
    Item {
        id: src
        width: 40; height: 30
        visible: false
        Rectangle { anchors.fill: parent; radius: 9; color: "#3366ff" }
        Rectangle { x: 22; y: 10; width: 12; height: 12; radius: 6; color: "#ee2211" }
    }"##;

fn scene(body: &str) -> String {
    format!("{SRC}\n{body}")
}

/// Finding 3: the source layer is the item's size times the devicePixelRatio.
#[test]
fn multieffect_dpr() {
    let body = scene(
        "MultiEffect { x: 10; y: 12; width: 40; height: 30; source: src; saturation: -0.5; brightness: 0.1 }\n\
         MultiEffect { x: 66; y: 12; width: 40; height: 30; source: src; colorization: 1; colorizationColor: \"#40c080\" }",
    );
    like_qt(&body, &[2.0, 1.5], 3, 0);
}

/// Finding 3: so is the mask layer.
#[test]
fn multieffect_mask_dpr() {
    let body = scene(
        "Item { id: mask; width: 40; height: 30; visible: false; layer.enabled: true\n\
             Rectangle { x: 4; y: 4; width: 32; height: 22; radius: 11; color: \"black\" } }\n\
         MultiEffect { x: 10; y: 12; width: 40; height: 30; source: src; maskEnabled: true; maskSource: mask }",
    );
    like_qt(&body, &[2.0, 1.5], 3, 0);
}

/// Finding 3: the padded source and each BlurItem are layers at the devicePixelRatio.
#[test]
fn multieffect_blur_dpr() {
    like_qt(&scene(BLURRED), &[2.0, 1.5], 3, 0);
}

const PADDED: &str = "MultiEffect { x: 40; y: 16; width: 40; height: 30; source: src; blurEnabled: true; blur: 0; blurMax: 16 }";
const BLURRED: &str = "MultiEffect { x: 40; y: 16; width: 40; height: 30; source: src; blurEnabled: true; blur: 0.6; blurMax: 16 }";

/// What is left of the blur at 1.5 with no tolerance, each pixel one level off. Held for
/// d0c7468b (the effect layers, #302): the rest is the layer, which is upside down against
/// Qt's. Qt keeps the top of the scene in row 0 of a layer's texture (QSGRhiLayer::grab
/// mirrors the projection), ours is bottom-up, and the same projection rounds otherwise on
/// the way to window coordinates: the padded source the first pass reads is one pixel off
/// Qt's already, (109,44), [232, 36, 24] against Qt's [232, 37, 25].
#[test]
fn multieffect_padded_source_without_tolerance() {
    like_qt(&scene(PADDED), &[1.5], 0, 1);
}

/// The blur of that layer: 1590 pixels, one level each.
#[test]
fn multieffect_blur_dpr_blurred_without_tolerance() {
    like_qt(&scene(BLURRED), &[1.5], 0, 1590);
}

/// A MultiEffect with blur or a shadow whose size is not whole, or whose source's is not:
/// its source and BlurItems are layers at the devicePixelRatio all the same.
#[test]
fn multieffect_blur_and_shadow_of_a_fractional_size_dpr() {
    let frac = ROUND.replace("width: 40; height: 30", "width: 40.4; height: 30.6");
    let blur = "blurEnabled: true; blur: 0.6; blurMax: 16";
    let shadow = "shadowEnabled: true; shadowBlur: 0.5; shadowHorizontalOffset: 4; \
                  shadowVerticalOffset: 4; shadowColor: \"#00ff00\"";
    for (src, size) in [
        (ROUND, "width: 40.5; height: 30.3"),
        (frac.as_str(), "width: 40.4; height: 30.6"),
        (ROUND, "width: 46; height: 26"),
    ] {
        let body = format!(
            "{src}\nMultiEffect {{ x: 12; y: 16; {size}; source: src; {blur} }}\n\
             MultiEffect {{ x: 68; y: 12; {size}; source: src; {shadow} }}"
        );
        like_qt(&body, &[1.25, 1.5, 2.0], 3, 0);
    }
}

/// Finding 4: a tap of the kernel is one device pixel, on layers at the devicePixelRatio.
#[test]
fn gaussian_blur_dpr() {
    let body = scene(
        "GaussianBlur { x: 12; y: 16; width: 40; height: 30; source: src; radius: 4; samples: 9; transparentBorder: true }\n\
         GaussianBlur { x: 68; y: 16; width: 40; height: 30; source: src; radius: 3; samples: 7 }",
    );
    like_qt(&body, &[2.0, 1.5], 3, 0);
}

/// Finding 4: DropShadow blurs with its radius and samples times the devicePixelRatio.
#[test]
fn drop_shadow_dpr() {
    let body = scene(
        "DropShadow { x: 12; y: 12; width: 40; height: 30; source: src; radius: 4; samples: 9; \
         horizontalOffset: 2; verticalOffset: 4; color: \"#c0000000\"; transparentBorder: true }\n\
         DropShadow { x: 68; y: 12; width: 40; height: 30; source: src; radius: 8; samples: 17; \
         spread: 0.3; color: \"#ff00ff\"; transparentBorder: true }",
    );
    like_qt(&body, &[2.0, 1.5], 3, 0);
    let inside = scene(
        "DropShadow { x: 12; y: 12; width: 40; height: 30; source: src; radius: 4; samples: 9; \
         horizontalOffset: 2; verticalOffset: 4; color: \"#c0000000\"; transparentBorder: false }",
    );
    like_qt(&inside, &[2.0, 1.5], 3, 0);
}

/// Finding 5: the cached shadow's layer is the effect's size times the devicePixelRatio.
#[test]
fn cached_rectangular_shadow_dpr() {
    let body = "RectangularShadow { x: 15; y: 15; width: 36; height: 30; radius: 12; blur: 2; \
                spread: 1; color: \"#ffffff\"; antialiasing: true; cached: true }\n\
                RectangularShadow { x: 70; y: 14; width: 36; height: 30; radius: 6; blur: 8; \
                offset: Qt.vector2d(2, 4); color: \"#ffcc00\"; antialiasing: true; cached: true }";
    like_qt(body, &[2.0, 1.5], 3, 0);
}

/// Finding 6: a radius that is not whole blurs as it is (the fallback shader with
/// spread = radius / kernel radius, in a border of that radius), not rounded.
#[test]
fn fractional_blur_radius() {
    let blur = "GaussianBlur { x: 12; y: 16; width: 40; height: 30; source: src; radius: RADIUS; samples: 9; transparentBorder: true }\n\
         DropShadow { x: 68; y: 14; width: 40; height: 30; source: src; radius: SHADOW; samples: 9; \
         horizontalOffset: 2; verticalOffset: 2; color: \"#ffffff\"; transparentBorder: true }";
    let body = |src: &str, radius: &str, shadow: &str| {
        format!(
            "{src}\n{}",
            blur.replace("RADIUS", radius).replace("SHADOW", shadow)
        )
    };
    like_qt(&body(ROUND, "2.6", "3.4"), &[1.0], 3, 0);
    like_qt(&body(SRC, "2.5", "3.25"), &[2.0], 3, 0);
}

/// Finding 7: a layer is (size * devicePixelRatio) rounded once: 40.4 x 30.6 at 1.5 is
/// 61 x 46 texels, not the 60 x 47 of the size rounded first. Desaturate already drew its
/// source at the devicePixelRatio, so only the rounding tells it from Qt.
#[test]
fn fractional_layer_size() {
    let src = "Item { id: src; width: 40.4; height: 30.6; visible: false\n\
            Rectangle { anchors.fill: parent; radius: 9; color: \"#3366ff\" }\n\
            Rectangle { x: 31; y: 5; width: 6; height: 6; radius: 3; color: \"#ffee00\" }\n\
            Rectangle { x: 5; y: 21; width: 6; height: 6; radius: 3; color: \"#ffffff\" }\n\
            Rectangle { x: 30; y: 20; width: 7; height: 7; radius: 3.5; color: \"#ee2211\" } }";
    let size = "x: 10; y: 12; width: 40.4; height: 30.6; source: src";
    like_qt(
        &format!("{src}\nDesaturate {{ {size}; desaturation: 0.5 }}"),
        &[1.5],
        3,
        0,
    );
    like_qt(
        &format!("{src}\nMultiEffect {{ {size}; brightness: 0.1 }}"),
        &[1.5],
        3,
        0,
    );
}
