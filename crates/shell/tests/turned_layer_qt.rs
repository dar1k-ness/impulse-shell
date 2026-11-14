//! A layer with an effect inside a turned item, against Qt itself (support/qt_window.rs) with
//! no tolerance, on the CPU path at scale 1 and on the GPU path on outputs of scale 1, 1.25,
//! 1.5 and 2. The tests hold how many pixels are not Qt's (docs/qt-differences.md, "Transforms
//! that go through a texture"): none where the button rests. The one such place of `qml/` is
//! modules/common/widgets/NavigationRailExpandButton.qml:16 (settings.qml:240): a
//! RippleButton, whose background is a layer under an OpacityMask (RippleButton.qml:144),
//! with `rotation: expanded ? 0 : -180` under a Behavior.

#[path = "support/qt_window.rs"]
mod qt_window;

const SIZE: (usize, usize) = (160, 60);
const DPRS: [f64; 4] = [1.0, 1.25, 1.5, 2.0];

/// The button at `x`, turned by `angle`: its round masked background and a mark off its
/// centre, which shows the turn.
fn button(x: f64, angle: f64) -> String {
    format!(
        "Item {{ x: {x}; y: 10; width: 40; height: 40; rotation: {angle}\n\
         Rectangle {{ id: bg{n}; anchors.fill: parent; radius: 9999; color: \"#4a4458\"\n\
         layer.enabled: true\n\
         layer.effect: OpacityMask {{ maskSource: Rectangle {{ width: bg{n}.width; \
         height: bg{n}.height; radius: 9999 }} }}\n\
         Rectangle {{ x: -4; y: 6; width: 20; height: 8; color: \"#e8def8\" }} }} }}\n",
        n = x as i64
    )
}

fn scene(buttons: &[(f64, f64)]) -> String {
    let mut s = String::from("Rectangle { anchors.fill: parent; color: \"#1d1b20\" }\n");
    for &(x, angle) in buttons {
        s += &button(x, angle);
    }
    s
}

/// `$what` on the CPU path and on the GPU path at each of DPRS, as two tests that hold how
/// many pixels are not Qt's: `$cpu`, and `$gpu` at each scale.
macro_rules! off_qt {
    ($(#[$doc:meta])* $name:ident, $cpu:expr, $gpu:expr, $what:expr) => {
        $(#[$doc])*
        mod $name {
            use super::*;

            #[test]
            fn cpu() {
                if let Some(n) = qt_window::cpu_off_qt(&scene(&$what), SIZE) {
                    assert_eq!(n, $cpu, "pixels that are not Qt's on the CPU path");
                }
            }

            #[test]
            fn gpu() {
                let by = qt_window::Scale::Output;
                if let Some(n) = qt_window::off_qt_by(by, &scene(&$what), SIZE, &DPRS) {
                    assert_eq!(n, $gpu, "pixels that are not Qt's at scales {DPRS:?}");
                }
            }
        }
    };
}

off_qt!(
    /// The two states the button rests in, expanded and collapsed, as Qt's; the 25 pixels are
    /// of the third button, half a pixel off the grid, where `qml/` has none.
    at_rest,
    25,
    [25, 0, 0, 0],
    [(10.0, 0.0), (60.0, -180.0), (110.5, -180.0)]
);

off_qt!(
    /// Frames of the Behavior between the two states: the ring of the round mask differs.
    while_turning,
    505,
    [488, 1039, 772, 1028],
    [(10.0, -30.0), (60.0, -90.0), (110.0, -157.5)]
);
