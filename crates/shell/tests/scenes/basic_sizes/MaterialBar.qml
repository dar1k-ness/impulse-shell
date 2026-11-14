import QtQuick
import QtQuick.Controls.Material
import QtQuick.Controls

// qml/modules/common/widgets/StyledIndeterminateProgressBar.qml: this file imports the
// Material style, so its ProgressBar is Material's (200x4) in a shell of the Basic style.
ProgressBar {
    indeterminate: true
    Material.accent: "#80a0ff"
}
