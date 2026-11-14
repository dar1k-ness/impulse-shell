//! An object that sets properties of a value type property (`sourceSize.width: 320`) takes
//! off the binding its type put on the whole value (`sourceSize: Qt.size(..)`):
//! QQmlObjectCreator::setupBindings removes it before it was ever evaluated, so the properties
//! the object does not set are the type's defaults and nothing follows the binding's inputs
//! (`qml/modules/common/widgets/SettingsWallpaperThumb.qml` over `StyledImage.qml`). The file
//! is run in Qt 6.12 and here, and the `console.log` lines must be the same.
#![cfg(feature = "interp")]

#[path = "support/qt_log.rs"]
mod qt_log;

const SRC: &str = r#"import QtQuick
Item {
    id: root
    width: 200; height: 100
    component Sized: Image { sourceSize: Qt.size(root.width * 1, root.height * 1) }
    component Lettered: Text { font: Qt.font({ pixelSize: root.width / 10, bold: true }) }
    component Parted: Image { sourceSize.width: root.width * 2; sourceSize.height: root.height * 2 }
    Sized { id: both; sourceSize.width: 320; sourceSize.height: 180 }
    Sized { id: one; sourceSize.width: 320 }
    Sized { id: bound; sourceSize.width: root.height * 3 }
    Sized { id: none }
    Lettered { id: text; font.pixelSize: 30 }
    Lettered { id: plain }
    Parted { id: part; sourceSize.width: 7 }
    function all() {
        return [both.sourceSize, one.sourceSize, bound.sourceSize, none.sourceSize,
                text.font.pixelSize, text.font.bold, plain.font.pixelSize, plain.font.bold,
                part.sourceSize].join(" ");
    }
    Component.onCompleted: {
        console.log("start", all());
        width = 300;
        height = 50;
        console.log("resized", all());
        Qt.quit();
    }
}
"#;

#[test]
fn parts_set_over_a_binding_of_the_whole() {
    if !qt_log::qt_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (qt, ours) = qt_log::both(dir.path(), "scene.qml", SRC);
    assert_eq!(
        qt,
        [
            "start QSize(320, 180) QSize(320, 0) QSize(300, 0) QSize(200, 100) 30 false 20 true QSize(7, 200)",
            "resized QSize(320, 180) QSize(320, 0) QSize(150, 0) QSize(300, 50) 30 false 30 true QSize(7, 100)",
        ]
    );
    assert_eq!(qt, ours);
}
