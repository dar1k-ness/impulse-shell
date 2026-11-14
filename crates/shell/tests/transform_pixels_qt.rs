//! `Item.transform` drawn as Qt 6.12 draws it, pixel for pixel and with no tolerance: the
//! scenes of transform_qt/scenes.rs shown by Qt Quick itself (OpenGL on llvmpipe,
//! support/qt_window.rs) and by us, on the GPU path at window scales 1, 1.25, 1.5 and 1.75 and
//! on the CPU path at scale 1. Qt draws a transformed subtree's geometry through its node's
//! matrix, so its edges are as sharp as an untransformed item's. tests/item_transform.rs
//! checks chosen points of simpler scenes at scale 1; these are the cases it leaves out:
//! fractions of a pixel, the origin of `scale` and `rotation` together with a list, rotation
//! about other axes, perspective and clips.

#[path = "support/qt_window.rs"]
mod qt_window;

const SIZE: (usize, usize) = (160, 100);
const DPRS: [f64; 4] = [1.0, 1.25, 1.5, 1.75];

/// A scene on both paths: `gpu` at every scale of DPRS, `cpu` at scale 1. `$cpu` and `$gpu`
/// are how many pixels are known not to be Qt's (docs/qt-differences.md, "Transforms that go
/// through a texture"), none when left out; a scene that comes closer to Qt fails as one
/// that goes further does.
macro_rules! scene {
    ($name:ident, $body:expr) => {
        scene!($name, 0, [0, 0, 0, 0], $body);
    };
    ($name:ident, $cpu:expr, $gpu:expr, $body:expr) => {
        mod $name {
            use super::qt_window::{Scale, cpu_off_qt, off_qt_by};

            #[test]
            fn gpu() {
                if let Some(n) = off_qt_by(Scale::Qt, $body, super::SIZE, &super::DPRS) {
                    assert_eq!(
                        n,
                        $gpu,
                        "pixels that are not Qt's at scales {:?}",
                        super::DPRS
                    );
                }
            }
            #[test]
            fn cpu() {
                if let Some(n) = cpu_off_qt($body, super::SIZE) {
                    assert_eq!(n, $cpu, "pixels that are not Qt's on the CPU path");
                }
            }
        }
    };
}

include!("transform_qt/scenes.rs");
