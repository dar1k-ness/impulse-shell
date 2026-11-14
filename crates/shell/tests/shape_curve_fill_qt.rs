//! The fill of a `Shape` with the CurveRenderer against Qt 6.12, pixel for pixel: the screen
//! corners of `RoundCorner.qml` (a quarter arc, in a layer and without one) and the wavy edge
//! of `SineCookie.qml` (a polyline).
#[path = "support/qt_window_shapes.rs"]
mod qt_window;

fn corner(x: i32, y: i32, sx: i32, sy: i32, start: i32, s: i32, layer: &str) -> String {
    format!(
        "Item {{ x: {x}; y: {y}; width: {s}; height: {s}\n\
           Shape {{ {layer} preferredRendererType: Shape.CurveRenderer\n\
             ShapePath {{ id: p{start}; strokeWidth: 0; fillColor: \"#000000\"\n\
               pathHints: ShapePath.PathSolid & ShapePath.PathNonIntersecting\n\
               startX: {sx}; startY: {sy}\n\
               PathAngleArc {{ moveToStart: false; centerX: {s} - {sx}; centerY: {s} - {sy}\n\
                 radiusX: {s}; radiusY: {s}; startAngle: {a}; sweepAngle: 90 }}\n\
               PathLine {{ x: {sx}; y: {sy} }} }} }} }}\n",
        a = if start == 270 { -90 } else { start }
    )
}

const LAYER: &str = "layer.enabled: true; layer.smooth: true;";

fn scene(s: i32, bg: &str, layer: &str) -> String {
    format!(
        "Rectangle {{ anchors.fill: parent; color: \"{bg}\" }}\n{}{}{}{}",
        corner(0, 0, 0, 0, 180, s, layer),
        corner(100 - s, 0, s, 0, 270, s, layer),
        corner(0, 100 - s, 0, s, 90, s, layer),
        corner(100 - s, 100 - s, s, s, 0, s, layer)
    )
}

#[test]
fn corners_23_white() {
    qt_window::like_qt(&scene(23, "#ffffff", LAYER), (100, 100), &[1.0], 0, 0);
}
#[test]
fn corners_23_colour() {
    qt_window::like_qt(&scene(23, "#d8c2bf", LAYER), (100, 100), &[1.0], 0, 0);
}
#[test]
fn corners_17_white() {
    qt_window::like_qt(&scene(17, "#ffffff", LAYER), (100, 100), &[1.0], 0, 0);
}
#[test]
fn plain_corners_23_white() {
    qt_window::like_qt(&scene(23, "#ffffff", ""), (100, 100), &[1.0], 0, 0);
}
#[test]
fn plain_corners_17_colour() {
    qt_window::like_qt(&scene(17, "#d8c2bf", ""), (100, 100), &[1.0], 0, 0);
}

/// `SineCookie.qml`'s outline: `sides` waves of `amplitude` around a circle.
fn cookie(bg: &str, layer: &str) -> String {
    format!(
        "Rectangle {{ anchors.fill: parent; color: \"{bg}\" }}\n\
         Shape {{ id: shape; x: 5; y: 7; width: 88; height: 88; {layer} preferredRendererType: Shape.CurveRenderer\n\
           ShapePath {{ strokeWidth: 0; fillColor: \"#c07a72\"\n\
             pathHints: ShapePath.PathSolid & ShapePath.PathNonIntersecting\n\
             PathPolyline {{ path: {{\n\
               var points = []; var cx = shape.width / 2; var cy = shape.height / 2\n\
               var steps = 360; var radius = 44 - 3\n\
               for (var i = 0; i <= steps; i++) {{\n\
                 var angle = (i / steps) * 2 * Math.PI\n\
                 var wave = Math.sin(angle * 12 + Math.PI / 2) * 3\n\
                 points.push(Qt.point(Math.cos(angle) * (radius + wave) + cx, Math.sin(angle) * (radius + wave) + cy))\n\
               }}\n\
               return points }} }} }} }}\n"
    )
}

#[test]
fn cookie_edge_white() {
    qt_window::like_qt(&cookie("#ffffff", ""), (100, 100), &[1.0], 0, 0);
}
#[test]
fn cookie_edge_colour() {
    qt_window::like_qt(&cookie("#73342d", ""), (100, 100), &[1.0], 0, 0);
}
#[test]
fn cookie_edge_in_a_layer() {
    qt_window::like_qt(&cookie("#ffffff", LAYER), (100, 100), &[1.0], 0, 0);
}
