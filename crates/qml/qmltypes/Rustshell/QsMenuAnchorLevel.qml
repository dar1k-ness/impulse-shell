import QtQuick
import QtQuick.Layouts
import Rustshell
import Rustshell.Widgets

// One level of a QsMenuAnchor menu: the entries of `handle` (a QsMenuHandle or a QsMenuEntry
// with children).
Rectangle {
    id: level

    required property var menuAnchor
    property int level: 0
    property var handle: null

    implicitWidth: Math.max(160, column.implicitWidth + 8)
    implicitHeight: column.implicitHeight + 8
    width: implicitWidth
    height: implicitHeight
    radius: 8
    color: menuAnchor.backgroundColor
    border.width: 1
    border.color: menuAnchor.borderColor

    QsMenuOpener {
        id: opener
        menu: level.handle
    }

    ColumnLayout {
        id: column
        x: 4
        y: 4
        width: level.width - 8
        spacing: 0

        Repeater {
            model: opener.children
            delegate: Item {
                id: entry
                required property var modelData
                readonly property bool separator: modelData.isSeparator
                readonly property bool enabled_: modelData.enabled
                Layout.fillWidth: true
                implicitHeight: separator ? 9 : 30
                implicitWidth: separator ? 0 : content.implicitWidth + 24

                Rectangle {
                    visible: entry.separator
                    anchors.verticalCenter: parent.verticalCenter
                    x: 8
                    width: parent.width - 16
                    height: 1
                    color: level.menuAnchor.separatorColor
                }

                Rectangle {
                    visible: !entry.separator
                    anchors.fill: parent
                    radius: 4
                    color: mouse.containsMouse && entry.enabled_ ? level.menuAnchor.highlightColor : "transparent"
                }

                RowLayout {
                    id: content
                    visible: !entry.separator
                    anchors.fill: parent
                    anchors.leftMargin: 12
                    anchors.rightMargin: 12
                    spacing: 8

                    Text {
                        visible: entry.modelData.buttonType !== QsMenuButtonType.None
                        text: entry.modelData.checkState === Qt.Checked
                            ? (entry.modelData.buttonType === QsMenuButtonType.RadioButton ? "\u25CF" : "\u2713")
                            : ""
                        Layout.preferredWidth: 12
                        color: entry.enabled_ ? level.menuAnchor.textColor : level.menuAnchor.disabledTextColor
                    }
                    IconImage {
                        visible: entry.modelData.icon !== ""
                        source: entry.modelData.icon
                        implicitSize: 16
                    }
                    Text {
                        Layout.fillWidth: true
                        text: entry.modelData.text
                        elide: Text.ElideRight
                        color: entry.enabled_ ? level.menuAnchor.textColor : level.menuAnchor.disabledTextColor
                    }
                    Text {
                        visible: entry.modelData.hasChildren
                        text: "\u203A"
                        color: entry.enabled_ ? level.menuAnchor.textColor : level.menuAnchor.disabledTextColor
                    }
                }

                MouseArea {
                    id: mouse
                    anchors.fill: parent
                    enabled: !entry.separator
                    hoverEnabled: true
                    onContainsMouseChanged: {
                        if (!containsMouse || !entry.enabled_)
                            return;
                        if (entry.modelData.hasChildren)
                            level.menuAnchor.openSubmenu(level.level, entry.modelData, level.y + column.y + entry.y);
                        else
                            level.menuAnchor.closeSubmenus(level.level);
                    }
                    onClicked: level.menuAnchor.activate(level.level, entry.modelData, level.y + column.y + entry.y)
                }
            }
        }
    }
}
