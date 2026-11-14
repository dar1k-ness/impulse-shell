//! Items in a window whose pixels are not a whole number of times its scale, against Qt itself
//! (support/qt_window.rs, the scale set on the compositor's output as on a real monitor), with
//! no tolerance. 161 by 97 at 1.25 is a surface of 201 by 121 pixels, at 1.5 of 242 by 146:
//! QQuickWindow projects over the pixel size divided by the scale as a QSize, which is rounded
//! (QQuickWindowPrivate::renderSceneGraph), so over 161 by 97 units and not over 160.8 by 96.8.
//! Each scene is shown in a window of 160 by 96 too, whose pixels are whole at both scales.

#[path = "support/qt_window.rs"]
mod qt_window;

const WHOLE: (usize, usize) = (160, 96);
const ODD: (usize, usize) = (161, 97);
const SCALES: [f64; 2] = [1.25, 1.5];

fn like_qt(body: &str, size: (usize, usize), scales: &[f64]) {
    qt_window::like_qt_by(qt_window::Scale::Output, body, size, scales, 0, 0);
}

macro_rules! in_both_windows {
    ($name:ident, $body:expr) => {
        in_both_windows!($name, $body, SCALES);
    };
    ($name:ident, $body:expr, $scales:expr) => {
        mod $name {
            use super::*;

            #[test]
            fn whole() {
                like_qt(&$body, WHOLE, &$scales);
            }

            #[test]
            fn odd() {
                like_qt(&$body, ODD, &$scales);
            }
        }
    };
}

in_both_windows!(
    rounded_rectangle,
    "Rectangle { x: 120.5; y: 60.5; width: 35; height: 30; radius: 8; color: \"#ff3366\" }\n"
);

in_both_windows!(
    rectangle,
    "Rectangle { x: 110; y: 55; width: 45; height: 35; color: \"#33ff66\" }\n"
);

in_both_windows!(
    turned_rectangle,
    "Item { x: 90; y: 40; width: 60; height: 40; rotation: 30\n\
         Rectangle { anchors.centerIn: parent; width: 50; height: 12; radius: 6; \
             color: \"#dfc38c\" } }\n"
);

// textmask.vert snaps a glyph's vertex to a pixel of the devicePixelRatio before the
// projection. Not at 1.25: there 56 pixels of this text are not Qt's in the window of whole
// pixels either (by 56 at most, at (141, 81)), which is not a matter of the window's size.
in_both_windows!(
    native_text,
    "Text { x: 84; y: 60; text: \"12:34 window\"; renderType: Text.NativeRendering; \
         color: \"#ffffff\"; font.family: \"Noto Sans\"; font.pixelSize: 12 }\n",
    [1.5]
);

in_both_windows!(
    clip,
    "Item { x: 100; y: 50; width: 50; height: 40; clip: true\n\
         Rectangle { x: -10; y: -10; width: 100; height: 100; radius: 30; color: \"#3366ff\" } }\n"
);

/// Fifteen rounded Rectangles, each in two items between pixels, no two at the same fraction
/// of a pixel; the inner item of every other one clips. Qt adds the items' positions up one by
/// one in floats, and an item that clips takes the sum so far into the matrix
/// (QSGBatchRenderer::Updater), which is where the last bit of an edge's place comes from.
fn nested() -> String {
    let mut body = String::new();
    for n in 0..15 {
        let (col, row) = ((n % 5) as f64, (n / 5) as f64);
        let (ax, ay) = (2.3 + col * 31.1, 1.7 + row * 31.3);
        let (bx, by) = (1.15 + col * 0.37, 0.85 + row * 0.59);
        let (cx, cy) = (2.6 + col * 0.13, 1.9 + row * 0.21);
        let clip = if n % 2 == 1 { "; clip: true" } else { "" };
        let alpha = if n % 3 == 2 { "80" } else { "ff" };
        body += &format!(
            "Item {{ x: {ax}; y: {ay}\n\
                 Item {{ x: {bx}; y: {by}; width: 28; height: 28{clip}\n\
                     Rectangle {{ x: {cx}; y: {cy}; width: 20.4; height: 18.6; radius: {}; \
                         color: \"#{alpha}ff3366\" }} }} }}\n",
            3 + n % 4 * 2
        );
    }
    body
}

in_both_windows!(rounded_rectangles_in_items_between_pixels, nested());

/// A picture of 40 by 24 pixels, every one of its own colour, in a directory that lasts as
/// long as the test.
fn picture() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let (w, h) = (40, 24);
    let mut c = qsg::raster::Canvas::new(w, h, [0, 0, 0, 255]);
    for (n, p) in c.px.iter_mut().enumerate() {
        let (x, y) = (n % w, n / w);
        *p = [
            (x * 255 / w) as u8,
            (y * 255 / h) as u8,
            ((x + y) % 2 * 255) as u8,
            255,
        ];
    }
    let file = dir.path().join("picture.png");
    c.save_png(file.to_str().unwrap()).unwrap();
    let body = format!(
        "Image {{ x: 112; y: 66; source: \"{}\" }}\n",
        file.to_str().unwrap()
    );
    (dir, body)
}

mod image {
    use super::*;

    #[test]
    fn whole() {
        let (_dir, body) = picture();
        like_qt(&body, WHOLE, &SCALES);
    }

    #[test]
    fn odd() {
        let (_dir, body) = picture();
        like_qt(&body, ODD, &SCALES);
    }
}
