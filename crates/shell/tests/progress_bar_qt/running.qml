import QtQuick
import QtQuick.Controls.Material
import QtQuick.Controls

// The two running bars of qml/: StyledIndeterminateProgressBar (Material, its own track) and
// WIndeterminateProgressBar (3 high, `background: null`), in the size Qt gives them.
Rectangle {
    width: 300; height: 64
    color: "#202020"
    ProgressBar { x: 10; y: 10; width: 240; indeterminate: true; Material.accent: "#80c0ff" }
    ProgressBar {
        x: 10; y: 30; width: 240; implicitHeight: 3
        indeterminate: true; background: null; Material.accent: "#80c0ff"
    }
}
