//! Pictures and the effects that blur on a monitor whose scale is not 1, against Qt itself
//! (support/qt_window.rs, the scale set on the compositor's output as on a real monitor), with
//! no tolerance: every device pixel is Qt's. Scale 1 is here too, so that it stays as it is.

#[path = "support/qt_window.rs"]
mod qt_window;
use qt_window::Scale;

const DPRS: [f64; 4] = [1.0, 1.25, 1.5, 2.0];
const SIZE: (usize, usize) = (96, 72);

/// An icon of the shell, drawn black.
const SVG: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../qml/assets/icons/spark-symbolic.svg"
);

fn like_qt(body: &str) {
    qt_window::like_qt_by(Scale::Output, body, SIZE, &DPRS, 0, 0);
}

/// The window hears of the output's scale after its scene was loaded, as a shell's panel does:
/// an Image loads its picture again then.
fn like_qt_scaled_later(body: &str) {
    qt_window::like_qt_by(Scale::OutputAfterLoad, body, SIZE, &DPRS[1..], 0, 0);
}

/// The shadow of the desktop clock (qml/modules/rusty/background/widgets/clock): its radius
/// and its samples are of the source's texture, which has the window's pixels.
#[test]
fn drop_shadow() {
    like_qt(
        "Item { id: src; x: 20; y: 14; width: 50; height: 40; visible: false\n\
           Canvas { anchors.fill: parent\n\
             onPaint: { var ctx = getContext(\"2d\"); ctx.reset(); ctx.fillStyle = \"#703030\";\n\
               ctx.beginPath(); ctx.arc(width/2, height/2, 18, 0, Math.PI*2); ctx.fill(); } } }\n\
         DropShadow { anchors.fill: src; source: src; radius: 8; samples: 17; \
           color: \"#80000000\"; transparentBorder: true }",
    );
}

#[test]
fn gaussian_blur() {
    like_qt(
        "Item { id: src; x: 20; y: 14; width: 50; height: 40; visible: false\n\
           Rectangle { x: 6; y: 6; width: 30; height: 24; radius: 8; color: \"#c04040\" } }\n\
         GaussianBlur { anchors.fill: src; source: src; radius: 6; samples: 13; \
           transparentBorder: true }",
    );
}

/// An SVG asked for at a size: Qt renders it at that size times the window's ratio.
fn sized() -> String {
    format!(
        "Rectangle {{ anchors.fill: parent; color: \"white\" }}\n\
         Image {{ property real sz: 19.5; x: 10; y: 10; width: 19.5; height: 19.5; \
           fillMode: Image.PreserveAspectFit; sourceSize.width: sz; sourceSize.height: sz; \
           source: \"file://{SVG}\" }}"
    )
}

/// SVGs asked for at no size, at a width and at a size that is not their shape, and what the
/// first tells of itself.
fn unsized_and_sized() -> String {
    format!(
        "Rectangle {{ anchors.fill: parent; color: \"white\" }}\n\
         Image {{ id: a; x: 10; y: 10; source: \"file://{SVG}\" }}\n\
         Image {{ x: 40; y: 10; sourceSize.width: 31; source: \"file://{SVG}\" }}\n\
         Image {{ x: 10; y: 40; width: 50; height: 20; sourceSize: Qt.size(21, 13); \
           source: \"file://{SVG}\" }}\n\
         Rectangle {{ x: 80; y: 0; width: a.implicitWidth; height: a.sourceSize.width; \
           color: \"red\" }}"
    )
}

/// What an Image tells of its size, as the widths of bars.
fn sizes_told() -> String {
    let bar = |y: usize, of: &str| {
        format!("Rectangle {{ x: 0; y: {y}; width: {of}; height: 4; color: \"red\" }}\n")
    };
    format!(
        "Rectangle {{ anchors.fill: parent; color: \"white\" }}\n\
         Image {{ id: a; x: 0; y: 40; source: \"file://{SVG}\" }}\n\
         Image {{ id: b; x: 40; y: 40; sourceSize.width: 31; source: \"file://{SVG}\" }}\n\
         {}{}{}{}{}{}{}{}",
        bar(0, "a.implicitWidth"),
        bar(4, "a.sourceSize.width"),
        bar(8, "a.width"),
        bar(12, "a.paintedWidth"),
        bar(16, "b.implicitWidth"),
        bar(20, "b.sourceSize.width"),
        bar(24, "b.sourceSize.height"),
        bar(28, "b.implicitHeight"),
    )
}

/// An icon of the top bar (qml/modules/common/widgets/CustomIcon.qml in LeftSidebarButton):
/// a picture asked for at the size of its item, under a ColorOverlay.
fn bar_icon() -> String {
    format!(
        "Rectangle {{ anchors.fill: parent; color: \"#404040\" }}\n\
         Item {{ x: 10; y: 10; width: 19.5; height: 19.5\n\
           Item {{ id: im; anchors.fill: parent; property real implicitSize: parent.height\n\
             readonly property real actualSize: Math.min(im.width, im.height)\n\
             implicitWidth: implicitSize; implicitHeight: implicitSize\n\
             Image {{ anchors.fill: parent; fillMode: Image.PreserveAspectFit; \
               source: \"file://{SVG}\"\n\
               sourceSize.width: im.actualSize; sourceSize.height: im.actualSize }} }}\n\
           ColorOverlay {{ anchors.fill: im; source: im; color: \"#e6e1e1\" }} }}"
    )
}

#[test]
fn svg_at_a_size() {
    like_qt(&sized());
}

#[test]
fn svg_at_a_size_scaled_later() {
    like_qt_scaled_later(&sized());
}

#[test]
fn svg_at_its_own_size() {
    like_qt(&unsized_and_sized());
}

#[test]
fn svg_at_its_own_size_scaled_later() {
    like_qt_scaled_later(&unsized_and_sized());
}

#[test]
fn sizes_an_image_tells() {
    like_qt(&sizes_told());
}

#[test]
fn sizes_an_image_tells_scaled_later() {
    like_qt_scaled_later(&sizes_told());
}

#[test]
fn icon_under_an_overlay() {
    like_qt(&bar_icon());
}

#[test]
fn icon_under_an_overlay_scaled_later() {
    like_qt_scaled_later(&bar_icon());
}

/// The wallpaper in Settings (qml/modules/common/widgets/SettingsWallpaperThumb.qml): its type
/// binds the whole `sourceSize` to the item's size, the object sets both parts of it, and Qt
/// loads the picture at what the object says.
#[test]
fn picture_of_a_type_that_binds_the_whole_size() {
    let wallpaper = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../qml/assets/images/default_wallpaper.png"
    );
    let body = format!(
        "    component Sized: Image {{\n        sourceSize: Qt.size(width, height)\n    }}\n    \
         Sized {{\n        x: 4; y: 4; width: 134; height: 75\n        \
         fillMode: Image.PreserveAspectCrop\n        sourceSize.width: 160\n        \
         sourceSize.height: 90\n        source: \"file://{wallpaper}\"\n    }}"
    );
    qt_window::like_qt_by(Scale::Output, &body, (144, 84), &DPRS, 0, 0);
}
