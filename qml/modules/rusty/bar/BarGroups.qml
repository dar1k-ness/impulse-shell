import QtQuick
import QtQuick.Layouts
import ri
import ri.services
import ri.modules.common

// The original bar groups ("media", "workspaces", "clock") one part of the horizontal bar holds,
// left to right, with a separator line between them on a borderless bar.
Row {
    id: root

    property list<string> groups: []
    // the original's form for narrow screens: 0 full, 1 shortened, 2 hella shortened.
    property int shortened: 0
    readonly property real sideModuleWidth: shortened === 2 ? Appearance.sizes.barCenterSideModuleWidthHellaShortened : shortened === 1 ? Appearance.sizes.barCenterSideModuleWidthShortened : Appearance.sizes.barCenterSideModuleWidth

    spacing: 4

    Repeater {
        model: root.groups

        delegate: Row {
            id: slot
            required property string modelData
            required property int index

            anchors.verticalCenter: parent?.verticalCenter
            spacing: root.spacing

            Rectangle {
                visible: slot.index > 0 && (Config.options?.bar.borderless ?? false)
                anchors.verticalCenter: parent.verticalCenter
                implicitWidth: 1
                implicitHeight: Appearance.sizes.baseBarHeight / 3
                color: Appearance.colors.colOutlineVariant
            }

            Loader {
                anchors.verticalCenter: parent.verticalCenter
                sourceComponent: slot.modelData === "media" ? mediaGroup : slot.modelData === "workspaces" ? workspacesGroup : clockGroup
            }
        }
    }

    Component {
        id: mediaGroup

        BarGroup {
            implicitWidth: root.sideModuleWidth

            Resources {
                alwaysShowAllResources: root.shortened === 2
                Layout.fillWidth: root.shortened === 2
            }

            Media {
                visible: root.shortened < 2
                Layout.fillWidth: true
            }
        }
    }

    Component {
        id: workspacesGroup

        BarGroup {
            Workspaces {
                id: workspacesWidget
                Layout.fillHeight: true
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

        MouseArea {
            implicitWidth: root.sideModuleWidth
            implicitHeight: clockGroupContent.implicitHeight

            onPressed: {
                GlobalStates.sidebarRightOpen = !GlobalStates.sidebarRightOpen;
            }

            BarGroup {
                id: clockGroupContent
                anchors.fill: parent

                ClockWidget {
                    showDate: (Config.options.bar.verbose && root.shortened < 2)
                    Layout.alignment: Qt.AlignVCenter
                    Layout.fillWidth: true
                }

                UtilButtons {
                    visible: (Config.options.bar.verbose && root.shortened === 0)
                    Layout.alignment: Qt.AlignVCenter
                }

                BatteryIndicator {
                    visible: (root.shortened < 2 && Battery.available)
                    Layout.alignment: Qt.AlignVCenter
                }
            }
        }
    }
}
