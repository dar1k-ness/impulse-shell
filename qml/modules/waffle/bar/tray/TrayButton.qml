pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Rustshell
import Rustshell.Services.SystemTray
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.waffle.looks
import ri.modules.waffle.bar

BarIconButton {
    id: root

    required property SystemTrayItem item
    property alias menuOpen: menu.visible
    readonly property bool barAtBottom: Config.options.waffles.bar.bottom
    iconSource: item.icon
    iconScale: 0
    Component.onCompleted: {
        root.iconScale = 1
    }
    Behavior on iconScale {
        animation: Looks.transition.enter.createObject(this)
    }

    onClicked: {
        item.activate();
    }

    altAction: () => {
        if (item.hasMenu) menu.open()
    }

    // This is lazy, but it's not like tray menus on Windoes are consistent...
    // TODO: Figure out how to do cascading menus then use a custom menu
    QsMenuAnchor {
        id: menu
        menu: root.item.menu
        anchor {
            adjustment: PopupAdjustment.ResizeY | PopupAdjustment.SlideX
            item: root
            gravity: root.barAtBottom ? Edges.Top : Edges.Bottom
            edges: root.barAtBottom ? Edges.Top : Edges.Bottom
        }
    }

    BarToolTip {
        extraVisibleCondition: root.shouldShowTooltip && !root.Drag.active
        text: TrayService.getTooltipForItem(root.item)
    }
}
