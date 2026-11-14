//! `transform: Translate`, against Qt itself (support/qt_window.rs) with no tolerance: every
//! device pixel is Qt's, on the CPU path at scale 1 and on the GPU path on outputs of scale 1,
//! 1.25, 1.5 and 2. The scenes are the three places of `qml/` with a `transform:`.

#[path = "support/qt_window.rs"]
mod qt_window;

const SIZE: (usize, usize) = (160, 96);
const DPRS: [f64; 4] = [1.0, 1.25, 1.5, 2.0];

/// `$body` on the CPU path and on the GPU path at each of `$dprs`, as two tests.
macro_rules! cpu_and_gpu_like_qt {
    ($(#[$doc:meta])* $name:ident, $size:expr, $dprs:expr, $body:expr) => {
        $(#[$doc])*
        mod $name {
            use super::*;

            #[test]
            fn cpu() {
                qt_window::cpu_like_qt(&$body, $size);
            }

            #[test]
            fn gpu() {
                qt_window::like_qt_by(qt_window::Scale::Output, &$body, $size, &$dprs, 0, 0);
            }
        }
    };
}

/// qml/modules/common/widgets/RippleButton.qml:176 and SecondaryTabButton.qml:135: the ripple
/// under a press at `at`, moved by half its size, inside the button's rounded layer.
fn ripple(at: &str) -> String {
    format!(
        "Rectangle {{ id: bg; x: 20; y: 28; width: 120; height: 40; radius: 12; color: \"#4a4458\"\n\
         layer.enabled: true\n\
         layer.effect: OpacityMask {{ maskSource: Rectangle {{ width: bg.width; \
         height: bg.height; radius: 12 }} }}\n\
         Item {{ id: ripple; {at}; width: 90; height: 90; opacity: 0.6\n\
         RadialGradient {{ anchors.fill: parent; gradient: Gradient {{\n\
         GradientStop {{ position: 0.0; color: \"#ffd8e4\" }}\n\
         GradientStop {{ position: 0.3; color: \"#ffd8e4\" }}\n\
         GradientStop {{ position: 0.5; color: Qt.rgba(1, 0.85, 0.9, 0) }} }} }}\n\
         transform: Translate {{ x: -ripple.width / 2; y: -ripple.height / 2 }} }} }}"
    )
}

/// qml/settings.qml:546: a page of the settings window sliding in, `shift` down.
fn page(shift: f64) -> String {
    format!(
        "Item {{ anchors.fill: parent; transform: Translate {{ y: {shift} }}\n\
         Rectangle {{ x: 10; y: 10; width: 60; height: 30; radius: 8; color: \"#4a4458\" }}\n\
         Text {{ x: 80; y: 16; renderType: Text.NativeRendering; \
         verticalAlignment: Text.AlignVCenter; font.family: \"Noto Sans\"; font.pixelSize: 15; \
         color: \"white\"; text: \"Ag 12\" }} }}"
    )
}

cpu_and_gpu_like_qt!(ripple_under_a_press, SIZE, DPRS, ripple("x: 40; y: 25"));
cpu_and_gpu_like_qt!(
    /// The press between whole pixels, as a pointer on a scaled output gives it.
    ripple_between_pixels,
    SIZE,
    DPRS,
    ripple("x: 40.4; y: 25.6")
);
cpu_and_gpu_like_qt!(
    /// Where the slide starts.
    page_shifted_by_14,
    SIZE,
    DPRS,
    page(14.0)
);
cpu_and_gpu_like_qt!(
    /// A frame of the slide.
    page_shifted_between_pixels,
    SIZE,
    DPRS,
    page(6.3)
);
