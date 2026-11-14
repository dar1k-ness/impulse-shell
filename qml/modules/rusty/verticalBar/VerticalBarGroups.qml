import QtQuick
import QtQuick.Layouts
import ri
import ri.services
import ri.modules.common
import ri.modules.rusty.bar as Bar

// The original bar groups ("media", "workspaces", "clock") one part of the vertical bar holds,
// top to bottom, with a separator line between them on a borderless bar.
Column {
    id: root

    property list<string> groups: []

    spacing: 4

    component GroupSeparator: Rectangle {
        Layout.leftMargin: Appearance.sizes.baseBarHeight / 3
        Layout.rightMargin: Appearance.sizes.baseBarHeight / 3
        Layout.fillWidth: true
        implicitHeight: 1
        color: Appearance.colors.colOutlineVariant
    }

    Repeater {
        model: root.groups

        delegate: Column {
            id: slot
            required property string modelData
            required property int index

            spacing: root.spacing

            Rectangle {
                visible: slot.index > 0 && (Config.options?.bar.borderless ?? false)
                x: Appearance.sizes.baseBarHeight / 3
                implicitWidth: Appearance.sizes.baseVerticalBarWidth - x * 2
                implicitHeight: 1
                color: Appearance.colors.colOutlineVariant
            }

            Loader {
                sourceComponent: slot.modelData === "media" ? mediaGroup : slot.modelData === "workspaces" ? workspacesGroup : clockGroup
            }
        }
    }

    Component {
        id: mediaGroup

        Bar.BarGroup {
            vertical: true
            padding: 8

            Resources {
                Layout.fillWidth: true
                Layout.fillHeight: false
            }

            GroupSeparator {}

            VerticalMedia {
                Layout.fillWidth: true
                Layout.fillHeight: false
            }
        }
    }

    Component {
        id: workspacesGroup

        Bar.BarGroup {
            vertical: true
            padding: 6

            Bar.Workspaces {
                vertical: true
                MouseArea {
                    // Right-click to toggle overview
                    anchors.fill: parent
                    acceptedButtons: Qt.RightButton

                    onPressed: event => {
                        if (event.button === Qt.RightButton) {
                            GlobalStates.overviewOpen = !GlobalStates.overviewOpen;
                        }
                    }
                }
            }
        }
    }

    Component {
        id: clockGroup

        Bar.BarGroup {
            vertical: true
            padding: 8

            VerticalClockWidget {
                Layout.fillWidth: true
                Layout.fillHeight: false
            }

            GroupSeparator {
                visible: Battery.available
            }

            BatteryIndicator {
                visible: Battery.available
                Layout.fillWidth: true
                Layout.fillHeight: false
            }
        }
    }
}
