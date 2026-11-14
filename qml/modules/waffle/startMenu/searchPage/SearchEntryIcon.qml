import QtQuick
import QtQuick.Layouts
import Rustshell
import ri
import ri.services
import ri.modules.common
import ri.modules.common.models
import ri.modules.common.functions
import ri.modules.common.widgets
import ri.modules.waffle.looks

Item {
    id: root
    required property LauncherSearchResult entry
    property int iconSize: 24
    implicitWidth: Math.max(iconSize, textIconLoader.implicitWidth)
    implicitHeight: iconSize
    Loader {
        anchors.centerIn: parent
        active: root.entry.iconType === LauncherSearchResult.IconType.System && root.entry.iconName !== ""
        sourceComponent: WAppIcon {
            implicitSize: root.iconSize
            iconName: root.entry.iconName
            tryCustomIcon: false
            animated: false
        }
    }
    Loader {
        id: textIconLoader
        anchors.centerIn: parent
        active: root.entry.iconType === LauncherSearchResult.IconType.Text
        sourceComponent: WText {
            text: root.entry.iconName
            font.pixelSize: root.iconSize
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }
    }
    Loader {
        anchors.centerIn: parent
        active: root.entry.iconType === LauncherSearchResult.IconType.Material || root.entry.iconType === LauncherSearchResult.IconType.None || root.entry.iconName === ""
        sourceComponent: FluentIcon {
            icon: root.entry.iconName ? WIcons.fluentFromMaterial(root.entry.iconName) : WIcons.guessIconForName(root.entry.name)
            implicitSize: root.iconSize
            animated: false
        }
    }
}
