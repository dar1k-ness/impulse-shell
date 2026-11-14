//! Text and icons under a quarter turn (`rotation: 90`, 180, 270, -90), against Qt itself
//! (support/qt_window.rs) with no tolerance: every device pixel is Qt's, on the CPU path at
//! scale 1 and on the GPU path on outputs of scale 1, 1.25, 1.5 and 2. The scenes are the
//! places of `qml/` that turn a text or an icon this way.

#[path = "support/qt_window.rs"]
mod qt_window;

const SIZE: (usize, usize) = (160, 96);
const DPRS: [f64; 4] = [1.0, 1.25, 1.5, 2.0];

/// qml/modules/common/widgets/StyledText.qml.
const TEXT: &str = "renderType: Text.NativeRendering; verticalAlignment: Text.AlignVCenter; \
    color: \"white\"; font.family: \"Noto Sans\"";

/// qml/modules/common/widgets/MaterialSymbol.qml of `size` pixels.
fn symbol(name: &str, size: u32, rest: &str) -> String {
    format!(
        "Text {{ renderType: Text.NativeRendering; verticalAlignment: Text.AlignVCenter; \
         color: \"white\"; font.hintingPreference: Font.PreferNoHinting; \
         font.family: \"Material Symbols Rounded\"; font.pixelSize: {size}; \
         font.variableAxes: ({{ \"FILL\": 0, \"opsz\": {size} }}); text: \"{name}\"; {rest} }}"
    )
}

fn cpu(body: &str) {
    qt_window::cpu_like_qt(body, SIZE);
}

fn gpu(body: &str, dprs: &[f64]) {
    qt_window::like_qt_by(qt_window::Scale::Output, body, SIZE, dprs, 0, 0);
}

/// `$body` on the CPU path and on the GPU path at each of `$dprs`, as two tests.
macro_rules! like_qt {
    ($(#[$doc:meta])* $name:ident, $dprs:expr, $body:expr) => {
        $(#[$doc])*
        mod $name {
            use super::*;

            #[test]
            fn cpu() {
                super::cpu(&$body);
            }

            #[test]
            fn gpu() {
                super::gpu(&$body, &$dprs);
            }
        }
    };
}

like_qt!(
    /// The arrow of an open settings row (qml/modules/common/widgets/SettingsExpandRow.qml:18),
    /// beside a line of text turned the same way.
    open_row_arrow,
    DPRS,
    format!(
        "{}\nText {{ x: 80; y: 30; font.pixelSize: 15; text: \"Ag 12\"; rotation: 90; {TEXT} }}",
        symbol("chevron_right", 22, "x: 20; y: 30; rotation: 90")
    )
);

like_qt!(
    /// The tray's overflow arrow on a vertical bar (qml/modules/rusty/bar/SysTray.qml:98): -90
    /// closed, 90 open, and 270 open on the inverted side.
    tray_arrow_on_a_vertical_bar,
    DPRS,
    [(10, -90), (60, 90), (110, 270)]
        .map(|(x, turn)| format!(
            "Item {{ x: {x}; y: 30; width: 34; height: 34\n{} }}",
            symbol(
                "expand_more",
                19,
                &format!(
                    "anchors.centerIn: parent; horizontalAlignment: Text.AlignHCenter; \
                     rotation: {turn}"
                )
            )
        ))
        .join("\n")
);

like_qt!(
    /// An arrow turned upside down, as the tray's on a horizontal bar when open (SysTray.qml:98)
    /// and the other arrows of `qml/` that open something.
    arrow_upside_down,
    DPRS,
    format!(
        "{}\nText {{ x: 80; y: 30; font.pixelSize: 15; text: \"Ag 12\"; rotation: 180; {TEXT} }}",
        symbol("expand_more", 19, "x: 20; y: 30; rotation: 180")
    )
);

like_qt!(
    /// An arrow and text turned upside down whose item is between whole pixels: half a pixel
    /// goes up on the screen, which is down in the turned item.
    upside_down_between_pixels,
    DPRS,
    format!(
        "{}\nText {{ x: 80.5; y: 30.5; font.pixelSize: 15; text: \"Ag 12\"; rotation: 180; {TEXT} }}",
        symbol("expand_more", 19, "x: 20.5; y: 30.5; rotation: 180")
    )
);

like_qt!(
    /// A turned icon and text whose item is between whole pixels.
    between_pixels,
    DPRS,
    format!(
        "{}\nText {{ x: 80.5; y: 30.5; font.pixelSize: 15; text: \"Ag 12\"; rotation: 90; {TEXT} }}",
        symbol("chevron_right", 22, "x: 20.5; y: 30.5; rotation: 90")
    )
);

like_qt!(
    /// The hour numbers of the clock's dial
    /// (qml/modules/rusty/background/widgets/clock/minuteMarks/BigHourNumbers.qml:23 and :38): four
    /// items turned by 90, 180, 270 and 360 around the dial's centre, each number turned back.
    dial_numbers,
    DPRS,
    format!(
        "Item {{ x: 34; y: 2; width: 92; height: 92\n\
         Repeater {{ model: 4\n\
         Item {{ id: numberItem; required property int index; anchors.fill: parent; \
         rotation: 360 / 4 * (index + 1)\n\
         Item {{ implicitWidth: 30; implicitHeight: implicitWidth; anchors.top: parent.top; \
         anchors.horizontalCenter: parent.horizontalCenter; anchors.topMargin: 2\n\
         Text {{ anchors.centerIn: parent; text: 12 / 4 * (numberItem.index + 1); \
         rotation: -numberItem.rotation; font.pixelSize: 24; {TEXT} }} }} }} }} }}"
    )
);

like_qt!(
    /// The translator's vertical annotation
    /// (qml/modules/common/widgets/SqueezedAnnotationStyledText.qml:69): a wrapped
    /// `Text.QtRendering` text as wide as its parent is high, turned by 90.
    vertical_annotation,
    DPRS,
    format!(
        "Item {{ x: 30; y: 8; width: 40; height: 80\n\
         Text {{ anchors.centerIn: parent; width: parent.height; wrapMode: Text.Wrap; \
         font.pixelSize: 14; text: \"Sphinx of vow\"; rotation: 90; {} }} }}",
        TEXT.replace("NativeRendering", "QtRendering")
    )
);
