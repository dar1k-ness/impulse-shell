//! Rounded rectangles in an item turned once, as the lines of the desktop clock's dial
//! (minuteMarks/Lines.qml) and its hands (MinuteHand.qml, HourHand.qml) at 12:34, against Qt
//! itself through OpenGL (support/qt_window.rs), with no tolerance, on our GPU path and on our
//! CPU path.

#[path = "support/qt_window.rs"]
mod qt_window;

const SIZE: (usize, usize) = (300, 300);
/// CookieClock.qml's 230 pixels on the colour of its cookie.
const FACE: &str = "Rectangle { anchors.fill: parent; color: \"#73342d\" }\n\
     Item { id: clock; x: 35; y: 35; width: 230; height: 230\n";

/// Lines.qml in its loader: twelve lines of 18 by 4 and sixty of 7 by 2.
fn lines(colour: &str) -> String {
    let ring = |n: u32, len: u32, thick: u32| {
        format!(
            "Repeater {{ model: {n}; Item {{ required property int index; \
                 rotation: 360 / {n} * index; anchors.fill: parent\n\
                 Rectangle {{ anchors {{ left: parent.left; verticalCenter: parent.verticalCenter; \
                     leftMargin: 12 }}\n\
                     implicitWidth: {len}; implicitHeight: {thick}; radius: implicitWidth / 2; \
                     color: {colour} }} }} }}\n"
        )
    };
    format!(
        "Item {{ anchors.fill: parent; anchors.margins: 10\n{}{}}}\n",
        ring(12, 18, 4),
        ring(60, 7, 2)
    )
}

/// The minute hand ("medium") at 34 minutes, the hour hand ("fill") at 12:34 and the dot.
const HANDS: &str = "Item { anchors.fill: parent; rotation: -90 + (360 / 60) * 34\n\
         Rectangle { anchors.verticalCenter: parent.verticalCenter; x: parent.width / 2 - 6; \
             width: 95; height: 12; radius: 6; color: \"#dfc38c\" } }\n\
     Item { anchors.fill: parent; rotation: -90 + (360 / 12) * (0 + 34 / 60)\n\
         Rectangle { anchors.verticalCenter: parent.verticalCenter; x: (parent.width - 20) / 2; \
             width: 72; height: 20; radius: 10; color: Qt.rgba(1, 0.706, 0.667, 1); \
             border.color: \"#ffb4aa\"; border.width: 4 } }\n\
     Rectangle { anchors.centerIn: parent; implicitWidth: 6; implicitHeight: 6; radius: 3; \
         color: \"#73342d\" }\n";

fn clock(inside: &str) -> String {
    format!("{FACE}{inside}}}\n")
}

#[test]
fn the_dial_s_lines_have_qt_s_pixels_on_the_gpu() {
    qt_window::like_qt(&clock(&lines("\"#844942\"")), SIZE, &[1.0], 0, 0);
}

#[test]
fn the_dial_s_lines_have_qt_s_pixels_on_the_cpu() {
    qt_window::cpu_like_qt(&clock(&lines("\"#844942\"")), SIZE);
}

#[test]
fn the_dial_s_lines_of_a_mixed_colour_have_qt_s_pixels_on_the_gpu() {
    let body = clock(&lines("Qt.rgba(0.5167, 0.2853, 0.2601, 1)"));
    qt_window::like_qt(&body, SIZE, &[1.0], 0, 0);
}

#[test]
fn the_hands_have_qt_s_pixels_on_the_gpu() {
    qt_window::like_qt(&clock(HANDS), SIZE, &[1.0], 0, 0);
}

#[test]
fn the_hands_have_qt_s_pixels_on_the_cpu() {
    qt_window::cpu_like_qt(&clock(HANDS), SIZE);
}

#[test]
fn the_clock_has_qt_s_pixels_on_the_gpu() {
    let body = clock(&format!("{}{HANDS}", lines("\"#844942\"")));
    qt_window::like_qt(&body, SIZE, &[1.0], 0, 0);
}
