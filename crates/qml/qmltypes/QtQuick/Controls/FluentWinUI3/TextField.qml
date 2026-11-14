import QtQuick
import QtQuick.Controls
import Rustshell

// QtQuick.Controls.FluentWinUI3 TextField, dark: Config.qml `controls.textfield` paddings and
// the dark/images/textfield-background*.png 9-patches (1px white border at alpha 19, fill at
// 15 / 21 hovered / 11 disabled, a 138 bottom line unless focused) plus the 2px FocusStroke.
TextField {
    id: control

    readonly property real __fill: !enabled ? 11 : activeFocus ? 15 : hovered ? 21 : 15
    // palette.accent of the Fluent theme: QKdeTheme (QT_QPA_PLATFORMTHEME=kde with
    // KDE_SESSION_VERSION >= 4) gives a white accent, the generic theme #308cc6.
    readonly property color __accent: {
        const themes = (Rustshell.env("QT_QPA_PLATFORMTHEME") ?? "").split(":");
        const kde = themes.includes("kde") || (Rustshell.env("XDG_CURRENT_DESKTOP") ?? "").toUpperCase().split(":").includes("KDE");
        return kde && parseInt(Rustshell.env("KDE_SESSION_VERSION") ?? "0") >= 4 ? "#ffffff" : "#308cc6";
    }

    implicitWidth: 160
    implicitHeight: 30
    topPadding: 5
    bottomPadding: 5
    leftPadding: 12
    rightPadding: 12
    color: control.palette.text
    selectionColor: control.palette.highlight
    selectedTextColor: control.palette.highlightedText
    placeholderTextColor: control.palette.placeholderText
    verticalAlignment: Text.AlignVCenter

    background: Item {
        implicitWidth: 160
        implicitHeight: 30
        Rectangle {
            anchors.fill: parent
            radius: 4
            color: Qt.rgba(1, 1, 1, control.__fill / 255)
            border.width: 1
            border.color: Qt.rgba(1, 1, 1, 19 / 255)
        }
        Rectangle {
            visible: !control.activeFocus
            x: 2
            width: parent.width - 4
            height: 1
            y: parent.height - 1
            color: Qt.rgba(1, 1, 1, 129 / 255)
        }
        Rectangle {
            visible: control.activeFocus
            width: parent.width
            height: 2
            y: parent.height - 2
            radius: 1
            color: control.__accent
        }
    }
}
