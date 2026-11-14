//! A Canvas in a window of a devicePixelRatio other than 1, against Qt itself
//! (support/qt_window.rs), with no tolerance. Qt paints a Canvas on an image of that ratio
//! (QQuickContext2DImageTexture::beginPainting), so its curves are as sharp as the screen's
//! pixels; painted at the item's size and stretched, the outline of the desktop clock and of
//! the lock screen's password dots was a soft one at monitor scales 1.25, 1.5 and 2.

#[path = "support/qt_window.rs"]
mod qt_window;

use qt_window::Scale;

/// A Canvas at (`x`, `y`) of `w` x `h` painting what ShapeCanvas.qml does (a blob of curves
/// under `scale` and `translate`, filled and outlined), a line, a disc and a cleared rect.
fn scene(x: f64, y: f64, w: f64, h: f64) -> String {
    format!(
        "Canvas {{ x: {x}; y: {y}; width: {w}; height: {h}\n\
             onPaint: {{\n\
                 var ctx = getContext(\"2d\");\n\
                 ctx.reset();\n\
                 ctx.fillStyle = \"#e6c8d0\";\n\
                 ctx.clearRect(0, 0, width, height);\n\
                 ctx.save();\n\
                 ctx.scale(width, height);\n\
                 ctx.translate(0.01, 0.02);\n\
                 ctx.beginPath();\n\
                 ctx.moveTo(0.5, 0.02);\n\
                 ctx.bezierCurveTo(1, 0, 1, 0.4, 0.96, 0.5);\n\
                 ctx.bezierCurveTo(1, 1, 0.6, 1, 0.5, 0.96);\n\
                 ctx.bezierCurveTo(0, 1, 0, 0.6, 0.04, 0.5);\n\
                 ctx.bezierCurveTo(0, 0, 0.4, 0, 0.5, 0.02);\n\
                 ctx.closePath();\n\
                 ctx.fill();\n\
                 ctx.strokeStyle = \"#80ffffff\";\n\
                 ctx.lineWidth = 0.03;\n\
                 ctx.stroke();\n\
                 ctx.restore();\n\
                 ctx.strokeStyle = \"#3070c0\";\n\
                 ctx.lineWidth = 1.5;\n\
                 ctx.beginPath();\n\
                 ctx.moveTo(4, 5);\n\
                 ctx.lineTo(width - 6, height - 9);\n\
                 ctx.stroke();\n\
                 ctx.fillStyle = \"#c0204060\";\n\
                 ctx.beginPath();\n\
                 ctx.arc(width * 0.3, height * 0.6, 7.5, 0, Math.PI * 2);\n\
                 ctx.fill();\n\
                 ctx.clearRect(width / 2 + 0.5, height / 2 - 2, 9, 4.5);\n\
             }} }}"
    )
}

const SIZE: (usize, usize) = (96, 72);

fn like_qt(body: &str, dpr: f64) {
    qt_window::like_qt_by(Scale::Output, body, SIZE, &[dpr], 0, 0);
}

/// The reference: at scale 1 the image's pixels are the window's.
#[test]
fn at_one() {
    like_qt(&scene(10.0, 8.0, 60.0, 50.0), 1.0);
}

#[test]
fn at_one_and_a_quarter() {
    like_qt(&scene(10.0, 8.0, 60.0, 50.0), 1.25);
}

#[test]
fn at_one_and_a_half() {
    like_qt(&scene(10.0, 8.0, 60.0, 50.0), 1.5);
}

#[test]
fn at_two() {
    like_qt(&scene(10.0, 8.0, 60.0, 50.0), 2.0);
}

/// 61 * 1.25 = 76.25 and 49 * 1.25 = 61.25: the image is 76 x 61 pixels over a rect a
/// quarter of a pixel larger, at 11 * 1.25 = 13.75.
#[test]
fn size_and_place_not_whole_at_one_and_a_quarter() {
    like_qt(&scene(11.0, 9.0, 61.0, 49.0), 1.25);
}

/// 60.5 units are a canvas window of 61 (QRectF::toRect), 92 pixels at 1.5.
#[test]
fn size_not_whole_in_units_at_one_and_a_half() {
    like_qt(&scene(10.0, 8.0, 60.5, 50.5), 1.5);
}
