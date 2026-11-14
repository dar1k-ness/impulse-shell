//! The differences from Qt 6.12 that docs/qt-differences.md names and that this crate holds:
//! each fixture is run in Qt and here, and each side must print what the document says it does.
#![cfg(feature = "interp")]

#[path = "support/qt_log.rs"]
mod qt_log;

/// The colours a style file binds to the palette, with a handler on each one's signal.
const COLOUR_SIGNALS: &str = r#"import QtQuick
import QtQuick.Controls
Item {
    id: root
    property var seen: []
    TextField {
        onColorChanged: root.seen.push("color")
        onPlaceholderTextColorChanged: root.seen.push("placeholderTextColor")
        onSelectionColorChanged: root.seen.push("selectionColor")
        onSelectedTextColorChanged: root.seen.push("selectedTextColor")
    }
    Label {
        onColorChanged: root.seen.push("Label.color")
        onLinkColorChanged: root.seen.push("Label.linkColor")
    }
    Component.onCompleted: {
        console.log("signals", seen.length, seen.join(","));
        Qt.quit();
    }
}
"#;

fn both(name: &str, src: &str) -> Option<(Vec<String>, Vec<String>)> {
    if !qt_log::qt_available() {
        return None;
    }
    let dir = tempfile::tempdir().unwrap();
    Some(qt_log::both(dir.path(), name, src))
}

#[test]
fn a_colour_bound_by_a_style_file_sends_no_signal_here_while_its_object_is_made() {
    let Some((qt, ours)) = both("colour_signals.qml", COLOUR_SIGNALS) else {
        return;
    };
    assert_eq!(qt, ["signals 2 placeholderTextColor,selectionColor"]);
    assert_eq!(ours, ["signals 0 "]);
}
