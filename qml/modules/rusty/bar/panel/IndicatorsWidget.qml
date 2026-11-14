import QtQuick
import QtQuick.Layouts
import ri
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.common.functions
import ri.modules.rusty.bar as Bar
import Impulse

RippleButton { // Right sidebar button with status indicators
    id: rightSidebarButton
    property bool vertical: parent?.vertical ?? false
    property bool zoneHovered: parent?.zoneHovered ?? false
    property int shortened: parent?.shortened ?? 0
    readonly property bool fillThickness: false
    width: parent?.width ?? implicitWidth
    height: parent?.height ?? implicitHeight

    implicitWidth: indicators.implicitWidth + (vertical ? 6 : 10) * 2
    implicitHeight: indicators.implicitHeight + (vertical ? 4 : 5) * 2

    buttonRadius: Appearance.rounding.full
    colBackground: zoneHovered ? Appearance.colors.colLayer1Hover : ColorUtils.transparentize(Appearance.colors.colLayer1Hover, 1)
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

    onPressed: GlobalStates.sidebarRightOpen = !GlobalStates.sidebarRightOpen

    // The margins animate as the original's do: each of the three on a horizontal bar (the original's BarContent),
    // the first and the third on a vertical one (the original's VerticalBarContent).
    GridLayout {
        id: indicators
        anchors.centerIn: parent
        property real realSpacing: rightSidebarButton.vertical ? 6 : 15
        columns: rightSidebarButton.vertical ? 1 : -1
        rowSpacing: 0
        columnSpacing: 0

        Revealer {
            vertical: rightSidebarButton.vertical
            reveal: Audio.sink?.audio?.muted ?? false
            Layout.fillHeight: !rightSidebarButton.vertical
            Layout.fillWidth: rightSidebarButton.vertical
            Layout.rightMargin: !rightSidebarButton.vertical && reveal ? indicators.realSpacing : 0
            Layout.bottomMargin: rightSidebarButton.vertical && reveal ? indicators.realSpacing : 0
            Behavior on Layout.rightMargin {
                animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
            }
            Behavior on Layout.bottomMargin {
                animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
            }
            MaterialSymbol {
                text: "volume_off"
                iconSize: Appearance.font.pixelSize.larger
                color: rightSidebarButton.colText
            }
        }
        Revealer {
            vertical: rightSidebarButton.vertical
            reveal: Audio.source?.audio?.muted ?? false
            Layout.fillHeight: !rightSidebarButton.vertical
            Layout.fillWidth: rightSidebarButton.vertical
            Layout.rightMargin: !rightSidebarButton.vertical && reveal ? indicators.realSpacing : 0
            Layout.bottomMargin: rightSidebarButton.vertical && reveal ? indicators.realSpacing : 0
            Behavior on Layout.rightMargin {
                animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
            }
            MaterialSymbol {
                text: "mic_off"
                iconSize: Appearance.font.pixelSize.larger
                color: rightSidebarButton.colText
            }
        }
        Bar.HyprlandXkbIndicator {
            vertical: rightSidebarButton.vertical
            Layout.alignment: rightSidebarButton.vertical ? Qt.AlignHCenter : Qt.AlignVCenter
            Layout.rightMargin: rightSidebarButton.vertical ? 0 : indicators.realSpacing
            Layout.bottomMargin: rightSidebarButton.vertical ? indicators.realSpacing : 0
            color: rightSidebarButton.colText
        }
        Revealer {
            vertical: rightSidebarButton.vertical
            reveal: Notifications.silent || Notifications.unread > 0
            Layout.fillHeight: !rightSidebarButton.vertical
            Layout.fillWidth: rightSidebarButton.vertical
            Layout.rightMargin: !rightSidebarButton.vertical && reveal ? indicators.realSpacing : 0
            Layout.bottomMargin: rightSidebarButton.vertical && reveal ? indicators.realSpacing : 0
            implicitHeight: reveal ? unreadCount.implicitHeight : 0
            implicitWidth: reveal ? unreadCount.implicitWidth : 0
            Behavior on Layout.rightMargin {
                animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
            }
            Behavior on Layout.bottomMargin {
                animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
            }
            Bar.NotificationUnreadCount {
                id: unreadCount
            }
        }
        MaterialSymbol {
            text: Network.materialSymbol
            iconSize: Appearance.font.pixelSize.larger
            color: rightSidebarButton.colText
        }
        MaterialSymbol {
            Layout.leftMargin: rightSidebarButton.vertical ? 0 : indicators.realSpacing
            Layout.topMargin: rightSidebarButton.vertical ? indicators.realSpacing : 0
            visible: BluetoothStatus.available
            text: BluetoothStatus.connected ? "bluetooth_connected" : BluetoothStatus.enabled ? "bluetooth" : "bluetooth_disabled"
            iconSize: Appearance.font.pixelSize.larger
            color: rightSidebarButton.colText
        }
    }
}
