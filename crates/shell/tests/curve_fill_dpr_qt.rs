//! The fill of a `Shape` with `preferredRendererType: Shape.CurveRenderer` on a screen whose
//! scale is not 1, against Qt itself (support/qt_window.rs, the scale set on the compositor's
//! output as on a real monitor), on the GPU with no tolerance: the two screen corners of
//! qml/modules/common/widgets/RoundCorner.qml.

#[path = "support/qt_window.rs"]
mod qt_window;

const CORNERS: &str = "Item { x: 20; y: 20; implicitWidth: 25; implicitHeight: 25\n\
     Shape { anchors.top: parent.top; anchors.left: parent.left; \
         preferredRendererType: Shape.CurveRenderer\n\
         ShapePath { strokeWidth: 0; fillColor: \"#ff3366\"; \
             pathHints: ShapePath.PathSolid & ShapePath.PathNonIntersecting; startX: 0; startY: 0\n\
             PathAngleArc { moveToStart: false; centerX: 25; centerY: 25; radiusX: 25; \
                 radiusY: 25; startAngle: 180; sweepAngle: 90 }\n\
             PathLine { x: 0; y: 0 } } } }\n\
 Item { x: 100; y: 50; implicitWidth: 25; implicitHeight: 25\n\
     Shape { anchors.bottom: parent.bottom; anchors.right: parent.right; \
         preferredRendererType: Shape.CurveRenderer\n\
         ShapePath { strokeWidth: 0; fillColor: \"#ff3366\"; \
             pathHints: ShapePath.PathSolid & ShapePath.PathNonIntersecting; startX: 25; startY: 25\n\
             PathAngleArc { moveToStart: false; centerX: 0; centerY: 0; radiusX: 25; \
                 radiusY: 25; startAngle: 0; sweepAngle: 90 }\n\
             PathLine { x: 25; y: 25 } } } }\n";

fn corners_in(size: (usize, usize), scale: f64) {
    let body = format!("import QtQuick.Shapes\n{CORNERS}");
    qt_window::like_qt_by(qt_window::Scale::Output, &body, size, &[scale], 0, 0);
}

fn corners_at(scale: f64) {
    corners_in((160, 96), scale);
}

/// The corners as RoundCorner.qml has them: 23 units, each Shape in a layer of its own. At 1.25
/// and 1.5 such a layer is a fraction of a pixel wide and differs from Qt for a plain Rectangle
/// too, so those scales are not here.
fn layered_corners_at(scale: f64) {
    let body = format!("import QtQuick.Shapes\n{CORNERS}")
        .replace("25", "23")
        .replace(
            "preferredRendererType",
            "layer.enabled: true; layer.smooth: true; preferredRendererType",
        );
    qt_window::like_qt_by(qt_window::Scale::Output, &body, (160, 96), &[scale], 0, 0);
}

#[test]
fn round_corners_at_one() {
    corners_at(1.0);
}

#[test]
fn round_corners_at_one_and_a_quarter() {
    corners_at(1.25);
}

#[test]
fn round_corners_at_one_and_a_half() {
    corners_at(1.5);
}

#[test]
fn round_corners_at_two() {
    corners_at(2.0);
}

#[test]
fn round_corners_in_a_window_of_a_fraction_of_a_unit_more_at_one_and_a_quarter() {
    corners_in((161, 97), 1.25);
}

#[test]
fn round_corners_in_a_window_of_a_fraction_of_a_unit_more_at_one_and_a_half() {
    corners_in((161, 97), 1.5);
}

#[test]
fn layered_round_corners_at_one() {
    layered_corners_at(1.0);
}

#[test]
fn layered_round_corners_at_two() {
    layered_corners_at(2.0);
}
