import QtQuick
import Rustshell
import Rustshell.Hyprland

// Rustshell's QsMenuAnchor shows a platform QMenu at its anchor. Natively the menu is a popup
// of the anchor item's window with cascading submenus: hovering an entry that has children
// opens its level beside it, all levels in the one popup surface.
PopupWindow {
    id: root

    property var menu: null
    // Rustshell's `visible` is read-only (open()/close() set it): it follows this flag.
    property bool _open: false
    // Opened submenus, outermost first: { handle: QsMenuEntry, y: offset of the level }.
    property var levels: []
    property color backgroundColor: "#2c2c2c"
    property color borderColor: "#3f3f3f"
    property color textColor: "#ffffff"
    property color disabledTextColor: "#7c7c7c"
    property color highlightColor: "#383838"
    property color separatorColor: "#454545"

    signal opened
    signal closed

    function open() {
        root.levels = [];
        root._open = true;
    }

    function close() {
        root._open = false;
    }

    // Open `entry`'s children as the level after `level`, aligned with the entry at `y`.
    function openSubmenu(level, entry, y) {
        const kept = root.levels.slice(0, level);
        const cur = root.levels[level];
        if (cur && cur.handle === entry)
            return;
        kept.push({ handle: entry, y: y });
        root.levels = kept;
    }

    // A click on `entry`: open its submenu, or trigger it and close the menu.
    function activate(level, entry, y) {
        if (!entry || !entry.enabled || entry.isSeparator)
            return;
        if (entry.hasChildren) {
            root.openSubmenu(level, entry, y);
            return;
        }
        entry.triggered();
        root.close();
    }

    function closeSubmenus(level) {
        if (root.levels.length > level)
            root.levels = root.levels.slice(0, level);
    }

    visible: root._open
    color: "transparent"
    anchor.window: anchor.item ? anchor.item.QsWindow.window : null
    implicitWidth: Math.max(1, row.implicitWidth)
    implicitHeight: Math.max(1, row.implicitHeight)

    onVisibleChanged: {
        if (visible) {
            root.opened();
        } else {
            root.levels = [];
            root.closed();
        }
    }

    HyprlandFocusGrab {
        active: root.visible
        windows: [root]
        onCleared: root.close()
    }

    Row {
        id: row
        spacing: 2

        QsMenuAnchorLevel {
            menuAnchor: root
            level: 0
            handle: root.visible ? root.menu : null
        }

        Repeater {
            model: root.levels
            delegate: Item {
                required property var modelData
                required property int index
                implicitWidth: sub.implicitWidth
                implicitHeight: sub.y + sub.implicitHeight
                width: implicitWidth
                height: implicitHeight

                QsMenuAnchorLevel {
                    id: sub
                    y: modelData.y
                    menuAnchor: root
                    level: index + 1
                    handle: root.visible ? modelData.handle : null
                }
            }
        }
    }
}
