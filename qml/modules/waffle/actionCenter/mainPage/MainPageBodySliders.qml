import QtQuick
import QtQuick.Layouts
import Rustshell
import ri
import ri.services
import ri.modules.common
import ri.modules.common.functions
import ri.modules.waffle.looks
import ri.modules.waffle.actionCenter
import ri.modules.waffle.actionCenter.volumeControl

ColumnLayout {
    id: root
    property var screen: root.QsWindow.window?.screen
    property var brightnessMonitor: Brightness.getMonitorForScreen(screen)
    spacing: 12

    RowLayout {
        spacing: 4

        WPanelIconButton {
            color: colBackground
            property real animationValue: root.brightnessMonitor?.brightness ?? 0
            rotation: animationValue * 180
            scale: 0.8 + animationValue * 0.2
            iconName: "weather-sunny"

            Behavior on animationValue {
                animation: Looks.transition.longMovement.createObject(this)
            }
        }
        
        WSlider {
            Layout.fillWidth: true
            value: root.brightnessMonitor?.brightness ?? 0
            scrollable: true
            onMoved: {
                root.brightnessMonitor?.setBrightness(value)
            }
        }

        WPanelIconButton {
            opacity: 0
        }
    }
    
    RowLayout {
        spacing: 4

        WPanelIconButton {
            iconName: WIcons.volumeIcon
            onClicked: Audio.toggleMute();
        }
        
        WSlider {
            Layout.fillWidth: true
            value: Audio.sink.audio.volume
            scrollable: true
            onMoved: {
                Audio.sink.audio.volume = value;
            }
        }

        WPanelIconButton {
            Component {
                id: volumeControlComp
                VolumeControl {}
            }
            onClicked: {
                ActionCenterContext.push(volumeControlComp)
            }
            contentItem: Item {
                anchors.centerIn: parent
                Row {
                    anchors.centerIn: parent
                    spacing: -1
                    FluentIcon {
                        anchors.verticalCenter: parent.verticalCenter
                        implicitSize: 18
                        icon: "options"
                    }
                    FluentIcon {
                        anchors.verticalCenter: parent.verticalCenter
                        implicitSize: 12
                        icon: "chevron-right"
                    }
                }
            }
        }
    }

}