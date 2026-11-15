import ri.modules.rusty.bar.weather
import QtQuick
import QtQuick.Layouts
import Rustshell
import Rustshell.Services.UPower
import ri
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.common.functions
import Impulse

Item { // Bar content region
    id: root

    property var screen: root.QsWindow.window?.screen
    property var brightnessMonitor: Brightness.getMonitorForScreen(screen)
    property real useShortenedForm: (Appearance.sizes.barHellaShortenScreenWidthThreshold >= screen?.width) ? 2 : (Appearance.sizes.barShortenScreenWidthThreshold >= screen?.width) ? 1 : 0
    readonly property int centerSideModuleWidth: (useShortenedForm == 2) ? Appearance.sizes.barCenterSideModuleWidthHellaShortened : (useShortenedForm == 1) ? Appearance.sizes.barCenterSideModuleWidthShortened : Appearance.sizes.barCenterSideModuleWidth

    // the original's own bar content. Not created while a panel of the layout replaces it.
    Loader {
        anchors.fill: parent
        active: !panelContent.active
        sourceComponent: Item {
            // Background shadow
            Loader {
                active: Config.options.bar.showBackground && Config.options.bar.cornerStyle === 1 && Config.options.bar.floatStyleShadow
                anchors.fill: barBackground
                sourceComponent: StyledRectangularShadow {
                    anchors.fill: undefined // The loader's anchors act on this, and this should not have any anchor
                    target: barBackground
                }
            }
            // Background
            Rectangle {
                id: barBackground
                anchors {
                    fill: parent
                    margins: Config.options.bar.cornerStyle === 1 ? (Appearance.sizes.hyprlandGapsOut) : 0 // idk why but +1 is needed
                }
                color: Config.options.bar.showBackground ? Appearance.colors.colLayer0 : "transparent"
                radius: Config.options.bar.cornerStyle === 1 ? Appearance.rounding.windowRounding : 0
                border.width: Config.options.bar.cornerStyle === 1 ? 1 : 0
                border.color: Appearance.colors.colLayer0Border
            }

            FocusedScrollMouseArea { // Left side | scroll to change brightness
                id: barLeftSideMouseArea

                anchors {
                    top: parent.top
                    bottom: parent.bottom
                    left: parent.left
                    right: middleSection.left
                }
                implicitWidth: leftSectionRowLayout.implicitWidth
                implicitHeight: Appearance.sizes.baseBarHeight

                onScrollDown: Brightness.decreaseBrightness()
                onScrollUp: Brightness.increaseBrightness()
                onMovedAway: GlobalStates.osdBrightnessOpen = false
                onPressed: event => {
                    if (event.button === Qt.LeftButton)
                        GlobalStates.sidebarLeftOpen = !GlobalStates.sidebarLeftOpen;
                }

                // Visual content
                ScrollHint {
                    reveal: barLeftSideMouseArea.hovered
                    icon: Hyprsunset.gamma === 100 ? "light_mode" : "wb_twilight"
                    tooltipText: Translation.tr("Scroll to change brightness")
                    side: "left"
                    anchors.left: parent.left
                    anchors.verticalCenter: parent.verticalCenter
                }

                RowLayout {
                    id: leftSectionRowLayout
                    anchors.fill: parent
                    spacing: 0

                    LeftSidebarButton { // Left sidebar button
                        id: leftSidebarButton
                        Layout.alignment: Qt.AlignVCenter
                        Layout.leftMargin: Appearance.rounding.screenRounding
                        colBackground: barLeftSideMouseArea.hovered ? Appearance.colors.colLayer1Hover : ColorUtils.transparentize(Appearance.colors.colLayer1Hover, 1)
                    }

                    BarGroups { // Groups that bar.layout puts on the left
                        id: leftGroups
                        visible: groups.length > 0
                        Layout.leftMargin: leftSidebarButton.visible ? 10 : Appearance.rounding.screenRounding
                        Layout.fillHeight: true
                        groups: BarGroupLayout.left
                        shortened: root.useShortenedForm
                    }

                    ActiveWindow {
                        Layout.leftMargin: 10 + (leftSidebarButton.visible || leftGroups.visible ? 0 : Appearance.rounding.screenRounding)
                        Layout.rightMargin: Appearance.rounding.screenRounding
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        visible: root.useShortenedForm === 0
                    }
                }
            }

            BarGroups { // Middle section
                id: middleSection
                anchors {
                    top: parent.top
                    bottom: parent.bottom
                    horizontalCenter: parent.horizontalCenter
                }
                groups: BarGroupLayout.center
                shortened: root.useShortenedForm
            }

            FocusedScrollMouseArea { // Right side | scroll to change volume
                id: barRightSideMouseArea

                anchors {
                    top: parent.top
                    bottom: parent.bottom
                    left: middleSection.right
                    right: parent.right
                }
                implicitWidth: rightSectionRowLayout.implicitWidth
                implicitHeight: Appearance.sizes.baseBarHeight

                onScrollDown: Audio.decrementVolume();
                onScrollUp: Audio.incrementVolume();
                onMovedAway: GlobalStates.osdVolumeOpen = false;
                onPressed: event => {
                    if (event.button === Qt.LeftButton) {
                        GlobalStates.sidebarRightOpen = !GlobalStates.sidebarRightOpen;
                    }
                }

                // Visual content
                ScrollHint {
                    reveal: barRightSideMouseArea.hovered
                    icon: "volume_up"
                    tooltipText: Translation.tr("Scroll to change volume")
                    side: "right"
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                }

                RowLayout {
                    id: rightSectionRowLayout
                    anchors.fill: parent
                    spacing: 5
                    layoutDirection: Qt.RightToLeft

                    RippleButton { // Right sidebar button
                        id: rightSidebarButton

                        Layout.alignment: Qt.AlignRight | Qt.AlignVCenter
                        Layout.rightMargin: Appearance.rounding.screenRounding
                        Layout.fillWidth: false

                        implicitWidth: indicatorsRowLayout.implicitWidth + 10 * 2
                        implicitHeight: indicatorsRowLayout.implicitHeight + 5 * 2

                        buttonRadius: Appearance.rounding.full
                        colBackground: barRightSideMouseArea.hovered ? Appearance.colors.colLayer1Hover : ColorUtils.transparentize(Appearance.colors.colLayer1Hover, 1)
                        colBackgroundHover: Appearance.colors.colLayer1Hover
                        colRipple: Appearance.colors.colLayer1Active
                        colBackgroundToggled: Appearance.colors.colSecondaryContainer
                        colBackgroundToggledHover: Appearance.colors.colSecondaryContainerHover
                        colRippleToggled: Appearance.colors.colSecondaryContainerActive
                        toggled: GlobalStates.sidebarRightOpen
                        property color colText: toggled ? Appearance.m3colors.m3onSecondaryContainer : Appearance.colors.colOnLayer0

                        Behavior on colText {
                            animation: Appearance.animation.elementMoveFast.colorAnimation.createObject(this)
                        }

                        onPressed: {
                            GlobalStates.sidebarRightOpen = !GlobalStates.sidebarRightOpen;
                        }

                        RowLayout {
                            id: indicatorsRowLayout
                            anchors.centerIn: parent
                            property real realSpacing: 15
                            spacing: 0

                            Revealer {
                                reveal: Audio.sink?.audio?.muted ?? false
                                Layout.fillHeight: true
                                Layout.rightMargin: reveal ? indicatorsRowLayout.realSpacing : 0
                                Behavior on Layout.rightMargin {
                                    animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
                                }
                                MaterialSymbol {
                                    text: "volume_off"
                                    iconSize: Appearance.font.pixelSize.larger
                                    color: rightSidebarButton.colText
                                }
                            }
                            Revealer {
                                reveal: Audio.source?.audio?.muted ?? false
                                Layout.fillHeight: true
                                Layout.rightMargin: reveal ? indicatorsRowLayout.realSpacing : 0
                                Behavior on Layout.rightMargin {
                                    animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
                                }
                                MaterialSymbol {
                                    text: "mic_off"
                                    iconSize: Appearance.font.pixelSize.larger
                                    color: rightSidebarButton.colText
                                }
                            }
                            HyprlandXkbIndicator {
                                Layout.alignment: Qt.AlignVCenter
                                Layout.rightMargin: indicatorsRowLayout.realSpacing
                                color: rightSidebarButton.colText
                            }
                            Revealer {
                                reveal: Notifications.silent || Notifications.unread > 0
                                Layout.fillHeight: true
                                Layout.rightMargin: reveal ? indicatorsRowLayout.realSpacing : 0
                                implicitHeight: reveal ? notificationUnreadCount.implicitHeight : 0
                                implicitWidth: reveal ? notificationUnreadCount.implicitWidth : 0
                                Behavior on Layout.rightMargin {
                                    animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
                                }
                                NotificationUnreadCount {
                                    id: notificationUnreadCount
                                }
                            }
                            MaterialSymbol {
                                text: Network.materialSymbol
                                iconSize: Appearance.font.pixelSize.larger
                                color: rightSidebarButton.colText
                            }
                            MaterialSymbol {
                                Layout.leftMargin: indicatorsRowLayout.realSpacing
                                visible: BluetoothStatus.available
                                text: BluetoothStatus.connected ? "bluetooth_connected" : BluetoothStatus.enabled ? "bluetooth" : "bluetooth_disabled"
                                iconSize: Appearance.font.pixelSize.larger
                                color: rightSidebarButton.colText
                            }
                        }
                    }

                    SysTray {
                        visible: root.useShortenedForm === 0
                        Layout.fillWidth: false
                        Layout.fillHeight: true
                        invertSide: Config?.options.bar.bottom
                    }

                    BarGroups { // Groups that bar.layout puts on the right
                        visible: groups.length > 0
                        Layout.fillHeight: true
                        groups: BarGroupLayout.right
                        shortened: root.useShortenedForm
                    }

                    Item {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                    }

                    // Weather
                    Loader {
                        Layout.leftMargin: 4
                        active: Config.options.bar.weather.enable

                        sourceComponent: BarGroup {
                            WeatherBar {}
                        }
                    }
                }
            }
        }
    }

    // A panel of the layout (layout.toml) on this screen replaces the built-in content.
    // ri-only: Qt never loads it (no panel container there), so `qs -c impulse-shell` is unchanged.
    Loader {
        id: panelContent
        anchors.fill: parent
        active: LayoutState.hasContainer("panel")
        source: "panel/PanelContent.qml"
    }
}
