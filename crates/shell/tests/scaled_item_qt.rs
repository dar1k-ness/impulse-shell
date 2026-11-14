//! An item's own `scale:`, against Qt itself (support/qt_window.rs) with no tolerance, on the
//! CPU path at scale 1 and on the GPU path on outputs of scale 1, 1.25, 1.5 and 2. The tests
//! hold how many pixels are not Qt's (docs/qt-differences.md, "Transforms that go through a
//! texture"). The scenes are what `qml/` scales while it rests: a pressed desktop
//! widget (AbstractBackgroundWidget.qml:32, 1.05), a pressed colour swatch of clipped quarters
//! (settings/QuickConfig.qml:114, 0.9) and a pressed preview tile, which clips
//! (SettingsPreviewTile.qml:40, 0.96).

#[path = "support/qt_window.rs"]
mod qt_window;

const SIZE: (usize, usize) = (160, 60);
const DPRS: [f64; 4] = [1.0, 1.25, 1.5, 2.0];
const PAGE: &str = "Rectangle { anchors.fill: parent; color: \"#1d1b20\" }\n";

/// `$what` on the CPU path and on the GPU path at each of DPRS, as two tests that hold how
/// many pixels are not Qt's: `$cpu`, and `$gpu` at each scale.
macro_rules! off_qt {
    ($(#[$doc:meta])* $name:ident, $cpu:expr, $gpu:expr, $what:expr) => {
        $(#[$doc])*
        mod $name {
            use super::*;

            #[test]
            fn cpu() {
                if let Some(n) = qt_window::cpu_off_qt(&format!("{PAGE}{}", $what), SIZE) {
                    assert_eq!(n, $cpu, "pixels that are not Qt's on the CPU path");
                }
            }

            #[test]
            fn gpu() {
                let by = qt_window::Scale::Output;
                if let Some(n) = qt_window::off_qt_by(by, &format!("{PAGE}{}", $what), SIZE, &DPRS) {
                    assert_eq!(n, $gpu, "pixels that are not Qt's at scales {DPRS:?}");
                }
            }
        }
    };
}

off_qt!(
    /// Rectangles, square and round, with a border and a child, grown and shrunk about their
    /// centres.
    rectangles,
    19,
    [0, 1, 1, 0],
    "Rectangle { x: 10; y: 10; width: 40; height: 30; radius: 8; color: \"#4a4458\"; scale: 1.05\n\
         border.width: 2; border.color: \"#e8def8\"\n\
         Rectangle { x: 6; y: 6; width: 12; height: 8; color: \"#ffffff\" } }\n\
     Rectangle { x: 60; y: 12; width: 30; height: 30; color: \"#d0bcff\"; scale: 0.9 }\n\
     Rectangle { x: 110; y: 12; width: 30; height: 30; radius: 15; color: \"#d0bcff\"; scale: 0.96\n\
         Rectangle { x: 10; y: 10; width: 10; height: 10; color: \"#381e72\" } }\n"
);

off_qt!(
    /// The swatch: quarters of a circle, each cut out by a plain Item, in an item that shrinks.
    clips_inside,
    0,
    [0, 49, 0, 0],
    "Item { x: 20; y: 8; width: 44; height: 44; scale: 0.9\n\
         Item { width: 22; height: 22; clip: true\n\
             Rectangle { width: 44; height: 44; radius: 22; color: \"#d0bcff\" } }\n\
         Item { x: 22; width: 22; height: 22; clip: true\n\
             Rectangle { x: -22; width: 44; height: 44; radius: 22; color: \"#ccc2dc\" } }\n\
         Item { y: 22; width: 22; height: 22; clip: true\n\
             Rectangle { y: -22; width: 44; height: 44; radius: 22; color: \"#efb8c8\" } }\n\
         Item { x: 22; y: 22; width: 22; height: 22; clip: true\n\
             Rectangle { x: -22; y: -22; width: 44; height: 44; radius: 22; color: \"#381e72\" } } }\n"
);

off_qt!(
    /// The tile: the shrunk item itself clips what is larger than it.
    clip_on_the_item,
    0,
    [0, 72, 0, 0],
    "Item { x: 20; y: 10; width: 60; height: 40; clip: true; scale: 0.96\n\
         Rectangle { x: -5; y: -5; width: 70; height: 50; color: \"#4a4458\" }\n\
         Rectangle { x: 40; y: 20; width: 30; height: 30; radius: 6; color: \"#e8def8\" } }\n"
);
